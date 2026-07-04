//! Operator-facing backup-credentials endpoints (private-server, admin SPA).
//!
//! These are thin wrappers over the `database::backups` models. They drive the
//! group backup lifecycle (`provisioning → ready` — no escrow step; Canopy owns
//! and rotates the passphrase), per-`(group,type)` schedule/retention editing,
//! the one-off "backup now" request, and the read-only stats panel.
//!
//! At onboarding Canopy creates the repo-passphrase Secret via the kube client
//! on `AppState` (generated for from-birth, operator-supplied for passphrase
//! mode). The jobs-side init op then creates/connects the kopia repo and is the
//! writer of the observable `status`/`last_init_error` transitions.

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{
	Uuid,
	backup::{BackupConfigStatus, BackupPlacement, BackupPurpose, BackupRepoMode, BackupType},
};
use database::backups::BackupCredentialIssuance;
use database::diesel_async::AsyncPgConnection;
use database::pg_duration::PgDuration;
use database::{
	BackupMaintenanceRun, BackupRecoveryVerification, BackupRepoStats, BackupRequest, BackupRun,
	BackupTypeDefault, NewBackupTypeDefault, NewServerGroupBackupConfig,
	NewServerGroupBackupSchedule, RecoveryVaultWrite, RetentionPolicy, ServerBackupCapability,
	ServerGroupBackupConfig, ServerGroupBackupSchedule, server_groups::ServerGroup,
	servers::Server,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::{AppState, RecoveryChallenge};

/// Secret key the from-birth init Job writes the generated passphrase under.
const REPO_PASSWORD_SECRET_KEY: &str = "password";

/// The recovery verification ceremony is due if the last one is older than this.
const RECOVERY_VERIFICATION_MAX_AGE_SECS: i64 = 365 * 24 * 3600;

/// A challenge older than this is stale (operator must request a fresh one).
const RECOVERY_CHALLENGE_TTL_SECS: i64 = 3600;

/// Cap on the recent-runs / maintenance lists in the stats panel.
const RECENT_LIMIT: i64 = 20;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(get))
		.routes(routes!(list))
		.routes(routes!(create))
		.routes(routes!(create_shared))
		.routes(routes!(upsert))
		.routes(routes!(probe))
		.routes(routes!(update))
		.routes(routes!(set_schedule))
		.routes(routes!(clear_schedule))
		.routes(routes!(group_schedules))
		.routes(routes!(type_defaults))
		.routes(routes!(set_type_default))
		.routes(routes!(create_repo))
		.routes(routes!(request_now))
		.routes(routes!(cancel_request))
		.routes(routes!(restore_window))
		.routes(routes!(allow_restore))
		.routes(routes!(disallow_restore))
		.routes(routes!(request_maintenance))
		.routes(routes!(cancel_maintenance))
		.routes(routes!(stats))
		.routes(routes!(capabilities))
		.routes(routes!(set_capability))
		.routes(routes!(delete))
		.routes(routes!(recovery_status))
		.routes(routes!(recovery_challenge))
		.routes(routes!(recovery_verify))
}

// ── Wire types ──────────────────────────────────────────────────────────────

/// A schedule and retention override for one backup type of a server group.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ScheduleView {
	/// Backup type this override applies to.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Expected seconds between scheduled backups of this type; null means
	/// manual-only (no schedule), which is distinct from an interval of zero.
	#[schema(value_type = Option<i64>, format = "int64")]
	pub expected_interval: Option<PgDuration>,
	/// Retention policy override; null means inherit the canopy-wide default
	/// for this backup type.
	pub retention: Option<RetentionPolicy>,
	/// Whether this override is allowed to fall below the organization's
	/// minimum retention floor.
	pub allow_below_floor: bool,
}

/// The full backup configuration and lifecycle state for a server group.
/// Never includes the repository passphrase.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackupConfigView {
	/// The server group this configuration belongs to.
	pub server_group_id: Uuid,
	/// Name of the S3 bucket backups are stored in.
	pub bucket: String,
	/// Key prefix within the bucket that this group's backups are stored
	/// under; empty means the bucket root.
	pub prefix: String,
	/// AWS IAM role ARN used to issue upload credentials to devices; grants
	/// write access only, not delete.
	pub target_role_arn: String,
	/// AWS IAM role ARN used for maintenance, inspection, and metrics
	/// collection on the backup repository; grants full access, including
	/// delete.
	pub maintenance_role_arn: String,
	/// AWS region the bucket is in, if set explicitly.
	pub region: Option<String>,
	/// How the repository's encryption passphrase is managed: generated
	/// automatically by Canopy, or supplied by the operator when connecting
	/// an existing repository.
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	/// Current lifecycle state of the backup repository: provisioning while
	/// it's being created, ready once backups and restores can run.
	#[schema(value_type = String)]
	pub status: BackupConfigStatus,
	/// Where the backup bucket lives: `external` if it was provisioned in the
	/// deployment's own cloud account, or `shared` if Canopy provisioned it
	/// automatically in a shared account. Distinguishes the two onboarding
	/// paths.
	#[schema(value_type = String)]
	pub placement: BackupPlacement,
	/// Error message from the most recent failed provisioning attempt, if
	/// any.
	pub last_init_error: Option<String>,
	/// When this configuration was created.
	pub created_at: Timestamp,
	/// When this configuration was last updated.
	pub updated_at: Timestamp,
	/// When an operator requested an out-of-cycle full maintenance run that
	/// hasn't started yet; null if none is pending.
	pub force_full_maintenance_at: Option<Timestamp>,
	/// Identity of the operator who requested the pending full-maintenance
	/// run (Tailscale login), if any.
	pub force_full_maintenance_by: Option<String>,
	/// Per-backup-type schedule and retention overrides for this group.
	pub schedules: Vec<ScheduleView>,
}

impl BackupConfigView {
	async fn build(
		db: &mut database::diesel_async::AsyncPgConnection,
		config: ServerGroupBackupConfig,
	) -> Result<Self> {
		let schedules = ServerGroupBackupSchedule::list_for_group(db, config.group_id)
			.await?
			.into_iter()
			.map(|s| ScheduleView {
				r#type: s.r#type,
				expected_interval: s.expected_interval,
				retention: s.retention.as_ref().and_then(RetentionPolicy::from_json),
				allow_below_floor: s.allow_below_floor,
			})
			.collect();
		Ok(Self {
			server_group_id: config.group_id,
			bucket: config.bucket,
			prefix: config.prefix,
			target_role_arn: config.target_role_arn,
			maintenance_role_arn: config.maintenance_role_arn,
			region: config.region,
			mode: config.mode,
			status: config.status,
			placement: config.placement,
			last_init_error: config.last_init_error,
			created_at: config.created_at,
			updated_at: config.updated_at,
			force_full_maintenance_at: config.force_full_maintenance_at,
			force_full_maintenance_by: config.force_full_maintenance_by,
			schedules,
		})
	}
}

/// Summary of one server group's backup configuration, for the fleet-wide
/// overview list.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackupConfigSummary {
	/// The server group this configuration belongs to.
	pub server_group_id: Uuid,
	/// Name of the S3 bucket backups are stored in.
	pub bucket: String,
	/// How the repository's encryption passphrase is managed.
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	/// Current lifecycle state of the backup repository.
	#[schema(value_type = String)]
	pub status: BackupConfigStatus,
	/// Error message from the most recent failed provisioning attempt, if
	/// any.
	pub last_init_error: Option<String>,
}

/// Identifies a server group.
#[derive(Deserialize, ToSchema)]
pub struct BackupsGroupArgs {
	/// The server group to operate on.
	pub server_group_id: Uuid,
}

/// Request to register a new external (bring-your-own-account) backup
/// configuration for a group.
#[derive(Deserialize, ToSchema)]
pub struct CreateBackupConfigArgs {
	/// The server group to configure.
	pub server_group_id: Uuid,
	/// Name of the S3 bucket to store backups in.
	pub bucket: String,
	/// Key prefix within the bucket to store this group's backups under;
	/// defaults to empty (bucket root).
	#[serde(default)]
	pub prefix: String,
	/// AWS IAM role ARN used to issue upload credentials to devices; must not
	/// grant delete permission.
	pub target_role_arn: String,
	/// AWS IAM role ARN used for maintenance, inspection, and metrics
	/// collection on the bucket; requires full S3 access (including delete)
	/// and CloudWatch access.
	pub maintenance_role_arn: String,
	/// AWS region the bucket is in, if not the default.
	pub region: Option<String>,
	/// How the repository's encryption passphrase should be managed:
	/// generate one automatically, or use one supplied by the operator.
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	/// Repository passphrase to use; required when `mode` is `passphrase`,
	/// ignored otherwise (Canopy generates one automatically).
	pub passphrase: Option<String>,
}

/// Request to declaratively create or update a group's backup configuration,
/// for automated/infrastructure-as-code callers. Always creates the
/// repository with an automatically generated passphrase; importing an
/// existing repository is only supported through the interactive setup
/// wizard. `bucket`/`prefix` identify the configuration and are immutable
/// after creation; the role ARNs and region are reconciled to the request on
/// every call. Schedule and retention are not configurable here — see the
/// `/backups/set_schedule` endpoint.
#[derive(Deserialize, ToSchema)]
pub struct UpsertBackupConfigArgs {
	/// The server group to configure.
	pub server_group_id: Uuid,
	/// Name of the S3 bucket to store backups in. Immutable once the
	/// configuration is created.
	pub bucket: String,
	/// Key prefix within the bucket to store this group's backups under;
	/// defaults to empty (bucket root). Immutable once the configuration is
	/// created.
	#[serde(default)]
	pub prefix: String,
	/// AWS IAM role ARN used to issue upload credentials to devices; must not
	/// grant delete permission.
	pub target_role_arn: String,
	/// AWS IAM role ARN used for maintenance, inspection, and metrics
	/// collection on the bucket; requires full S3 access (including delete)
	/// and CloudWatch access.
	pub maintenance_role_arn: String,
	/// AWS region the bucket is in, if not the default.
	pub region: Option<String>,
}

/// Request to update a group's mutable backup configuration fields.
#[derive(Deserialize, ToSchema)]
pub struct UpdateBackupConfigArgs {
	/// The server group to update.
	pub server_group_id: Uuid,
	/// New AWS region for the bucket, or null to clear it. Changing the
	/// region effectively migrates the backup repository to a new location.
	/// Other configuration fields (bucket, roles, mode) cannot be changed
	/// through this endpoint.
	pub region: Option<String>,
}

/// Request to set (or override) the schedule and retention for one backup
/// type of a server group.
#[derive(Deserialize, ToSchema)]
pub struct SetScheduleArgs {
	/// The server group to configure.
	pub server_group_id: Uuid,
	/// Backup type this schedule and retention apply to.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Expected seconds between scheduled backups of this type; null means
	/// manual-only (no schedule), which is distinct from an interval of zero.
	#[schema(value_type = Option<i64>, format = "int64")]
	pub expected_interval: Option<PgDuration>,
	/// Retention policy to apply; null means inherit the canopy-wide default
	/// for this backup type. A specified policy is validated against the
	/// organization's minimum retention floor unless `allow_below_floor` is
	/// set.
	pub retention: Option<RetentionPolicy>,
	/// Allows this override to specify retention below the organization's
	/// minimum retention floor. Defaults to false.
	#[serde(default)]
	pub allow_below_floor: bool,
}

/// Identifies a one-off backup or restore request for a server.
#[derive(Deserialize, ToSchema)]
pub struct RequestArgs {
	/// The server to back up or restore.
	pub server_id: Uuid,
	/// Backup type to run.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Why this run is requested: `backup` to write new data, or `restore` to
	/// read existing data.
	#[schema(value_type = String)]
	pub purpose: BackupPurpose,
}

/// A pending one-off backup or restore request.
#[derive(Serialize, ToSchema)]
pub struct PendingRequestRow {
	/// The server the request is for.
	pub server_id: Uuid,
	/// Backup type requested.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Why this run was requested: `backup` to write new data, or `restore`
	/// to read existing data.
	#[schema(value_type = String)]
	pub purpose: BackupPurpose,
	/// When the request was made.
	pub requested_at: Timestamp,
	/// Identity of the operator who made the request (Tailscale login), if
	/// known.
	pub requested_by: Option<String>,
}

/// Backup statistics and activity for a server group.
#[derive(Serialize, ToSchema)]
pub struct BackupStatsView {
	/// Cached repository-level statistics, if available.
	pub stats: Option<BackupRepoStats>,
	/// The most recent backup runs across the group's member servers.
	pub recent_runs: Vec<BackupRun>,
	/// The most recent maintenance runs for the group's backup repository.
	pub recent_maintenance: Vec<BackupMaintenanceRun>,
	/// One-off backup/restore requests awaiting pickup.
	pub pending_requests: Vec<PendingRequestRow>,
	/// Backup types each member server has advertised it can run (with their
	/// enabled state), so the "back up now" panel can offer the right types per
	/// server and grey out servers that have declared none.
	pub capabilities: Vec<ServerBackupCapabilityView>,
	/// Member servers whose restore window is currently open, so the panel can
	/// show which servers may restore right now and until when. Servers with no
	/// open window are omitted.
	pub restore_windows: Vec<RestoreWindowRow>,
	/// Total raw S3 bytes uploaded to the bucket by the group's device backup
	/// runs so far this calendar month (UTC). Repo maintenance/inspection
	/// traffic isn't tallied anywhere, so this undercounts the bucket's
	/// actual monthly S3 traffic.
	pub s3_month_sent_bytes: i64,
	/// Total raw S3 bytes downloaded from the bucket by the group's device
	/// backup runs so far this calendar month (UTC). Same undercount caveat
	/// as `s3_month_sent_bytes`.
	pub s3_month_received_bytes: i64,
}

/// A server's currently-open restore window, as shown in the group stats.
#[derive(Serialize, ToSchema)]
pub struct RestoreWindowRow {
	/// The server the window applies to.
	pub server_id: Uuid,
	/// When the window closes; the server may mint restore credentials until
	/// then.
	pub allowed_until: Timestamp,
	/// Who opened the window (Tailscale login), if known.
	pub allowed_by: Option<String>,
}

/// A single server's restore-window state, for the server detail page.
#[derive(Serialize, ToSchema)]
pub struct RestoreWindowView {
	/// When the restore window closes, or `null` if restores are not currently
	/// allowed for this server.
	pub allowed_until: Option<Timestamp>,
	/// Who opened the current window (Tailscale login), if known.
	pub allowed_by: Option<String>,
}

impl RestoreWindowView {
	/// The window as reported to operators: reflects the stored values only
	/// while the window is still open, so an expired window reads as closed.
	fn of(server: &database::servers::Server) -> Self {
		if server.restore_allowed() {
			Self {
				allowed_until: server.restore_allowed_until,
				allowed_by: server.restore_allowed_by.clone(),
			}
		} else {
			Self {
				allowed_until: None,
				allowed_by: None,
			}
		}
	}
}

/// Effective scheduled interval (seconds) for a `(group, type)`: the per-group
/// override if set, else the canopy-wide default. `None` = manual-only.
async fn effective_interval_secs(
	conn: &mut AsyncPgConnection,
	group_id: Uuid,
	ty: &BackupType,
) -> Result<Option<i64>> {
	let over = ServerGroupBackupSchedule::get(conn, group_id, ty).await?;
	let def = BackupTypeDefault::get(conn, ty).await?;
	Ok(over
		.as_ref()
		.and_then(|s| s.expected_interval)
		.or_else(|| def.as_ref().and_then(|d| d.default_interval))
		.map(|pg| pg.0.as_secs()))
}

/// `Some(issued_at)` when a backup looks in flight: a backup credential is still
/// within its validity window (`now < expires_at`) and no run has been reported
/// since it was issued. `None` otherwise. The window is the credential lifetime
/// itself — once the creds expire the device can no longer be using them.
fn processing_since(
	now: Timestamp,
	issuance: Option<(Timestamp, Timestamp)>,
	last_report_at: Option<Timestamp>,
) -> Option<Timestamp> {
	let (issued, expires) = issuance?;
	if now >= expires {
		return None;
	}
	match last_report_at {
		Some(reported) if reported >= issued => None,
		_ => Some(issued),
	}
}

/// Next expected backup for one `(server, type)`: the server's own last success
/// plus the interval, or `now` (overdue) if scheduled-but-never-run. `None` when
/// disabled or manual-only.
fn next_backup_at(
	enabled: bool,
	interval_secs: Option<i64>,
	last_success_at: Option<Timestamp>,
	now: Timestamp,
) -> Option<Timestamp> {
	if !enabled {
		return None;
	}
	let secs = interval_secs?;
	Some(match last_success_at {
		Some(last) => Timestamp::from_second(last.as_second() + secs).unwrap_or(now),
		None => now,
	})
}

/// One backup type a server has advertised support for, whether the operator
/// has enabled it, and its most recent activity. Enabling a capability is
/// what makes the server eligible for issued backup credentials and
/// scheduled runs of that type.
#[derive(Serialize, ToSchema)]
pub struct ServerBackupCapabilityView {
	/// The server this capability belongs to.
	pub server_id: Uuid,
	/// Backup type this capability describes.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Whether the operator has enabled scheduled backups of this type for
	/// this server.
	pub enabled: bool,
	/// Identifier of this server and type's most recent successful backup,
	/// if any.
	pub latest_snapshot_id: Option<String>,
	/// When that snapshot was reported.
	pub latest_snapshot_at: Option<Timestamp>,
	/// Bytes uploaded by that run, if reported.
	pub latest_snapshot_bytes: Option<i64>,
	/// When this server's next backup of this type is expected: the server's own
	/// last success plus the effective interval, or "now" (overdue) if it's
	/// scheduled but has never succeeded. `None` for disabled or manual-only
	/// (no-interval) types. Per-server, so a lagging member isn't masked by a
	/// freshly-backed-up sibling.
	pub next_backup_at: Option<Timestamp>,
	/// `Some(issued_at)` when a backup of this type appears to be in flight:
	/// backup credentials were issued and are still valid, and no run has
	/// been reported since they were issued. `None` otherwise. Lets the UI
	/// show a "backing up…" state.
	pub processing_since: Option<Timestamp>,
}

/// Identifies a server.
#[derive(Deserialize, ToSchema)]
pub struct ServerArgs {
	/// The server to operate on.
	pub server_id: Uuid,
}

/// Request to enable or disable a server's backup capability for one type.
#[derive(Deserialize, ToSchema)]
pub struct SetCapabilityArgs {
	/// The server to update.
	pub server_id: Uuid,
	/// Backup type to enable or disable.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Whether scheduled backups of this type should be enabled for this
	/// server.
	pub enabled: bool,
}

// ── Handlers ──────────────────────────────────────────────────────────────

/// Get a group's backup configuration.
///
/// Returns the full configuration and lifecycle state for a server group.
/// Returns `null` with a 200 status if the group has no backup configuration
/// yet; 404 only if the group itself doesn't exist.
#[utoipa::path(
	post,
	path = "/get",
	operation_id = "backups_get",
	tag = "backups",
	security(("tailscale-user" = [])),
	request_body = BackupsGroupArgs,
	responses(
		(status = 200, body = Option<BackupConfigView>),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get(
	State(state): State<AppState>,
	Json(args): Json<BackupsGroupArgs>,
) -> Result<Json<Option<BackupConfigView>>> {
	let mut conn = state.db.get().await?;
	// 404 if the group itself is missing.
	ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;
	let config = ServerGroupBackupConfig::get(&mut conn, args.server_group_id).await?;
	let view = match config {
		Some(c) => Some(BackupConfigView::build(&mut conn, c).await?),
		None => None,
	};
	Ok(Json(view))
}

/// List all groups with a backup configuration.
///
/// Returns one summary row per configured group: the bucket, how the
/// repository passphrase originated, the provisioning status, and the
/// last provisioning error if any.
#[utoipa::path(
	post,
	path = "/list",
	operation_id = "backups_list",
	tag = "backups",
	security(("tailscale-user" = [])),
	responses((status = 200, body = Vec<BackupConfigSummary>)),
)]
pub async fn list(
	State(state): State<AppState>,
	_body: Json<serde_json::Value>,
) -> Result<Json<Vec<BackupConfigSummary>>> {
	let mut conn = state.db.get().await?;
	let rows = ServerGroupBackupConfig::list(&mut conn).await?;
	Ok(Json(
		rows.into_iter()
			.map(|c| BackupConfigSummary {
				server_group_id: c.group_id,
				bucket: c.bucket,
				mode: c.mode,
				status: c.status,
				last_init_error: c.last_init_error,
			})
			.collect(),
	))
}

/// Register a new external (bring-your-own-account) backup configuration.
///
/// This only records the configuration, in a provisioning state; it does not
/// create the backup repository itself — call the `/backups/create_repo`
/// endpoint next. Returns 409 if the group already has a backup
/// configuration, or 404 if the group doesn't exist.
#[utoipa::path(
	post,
	path = "/create",
	operation_id = "backups_create",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = CreateBackupConfigArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 400, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<CreateBackupConfigArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;

	let kube = state.kube.as_ref().ok_or_else(|| {
		AppError::Upstream("secret store not configured; cannot create repo passphrase".into())
	})?;

	// Canopy owns the passphrase Secret for both modes: generate one for
	// from-birth, take the operator's for passphrase mode.
	let passphrase = match args.mode {
		BackupRepoMode::FromBirth => commons_servers::backup_secrets::generate_passphrase(),
		BackupRepoMode::Passphrase => {
			let p = args.passphrase.clone().unwrap_or_default();
			if p.is_empty() {
				return Err(AppError::BadRequest(
					"passphrase mode requires a passphrase".into(),
				));
			}
			p
		}
	};

	// Stored under a deterministic, group-keyed Secret name.
	let repo_password_ref = format!("backup-repo-{}", args.server_group_id);

	// Insert the config first (PK-guards a duplicate create → 409), then store
	// the passphrase Secret Canopy owns.
	let config = ServerGroupBackupConfig::insert(
		&mut conn,
		NewServerGroupBackupConfig {
			group_id: args.server_group_id,
			bucket: args.bucket,
			prefix: args.prefix,
			target_role_arn: args.target_role_arn,
			maintenance_role_arn: args.maintenance_role_arn,
			region: args.region,
			repo_password_ref: repo_password_ref.clone(),
			status: BackupConfigStatus::Provisioning,
			mode: args.mode,
			placement: BackupPlacement::External,
		},
	)
	.await?;
	// Roll back the config row if the Secret can't be created, so onboarding is
	// all-or-nothing — a failed Secret create must not leave a half-created config
	// stuck in `provisioning` with no passphrase.
	if let Err(e) = kube
		.create_password(&repo_password_ref, REPO_PASSWORD_SECRET_KEY, &passphrase)
		.await
	{
		let _ = ServerGroupBackupConfig::delete(&mut conn, args.server_group_id).await;
		return Err(e);
	}

	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Request to onboard a group onto Canopy's shared-account backups. Canopy
/// fills in the bucket name and IAM roles automatically.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSharedBackupConfigArgs {
	/// The server group to onboard.
	pub server_group_id: Uuid,
	/// AWS region override for the bucket; if omitted, Canopy uses its
	/// default region for shared-account backups.
	#[serde(default)]
	pub region: Option<String>,
}

/// Onboard a group onto Canopy's shared-account backups.
///
/// Use this for deployments that don't have their own AWS account. Canopy
/// generates a bucket name automatically, generates and stores the repository
/// passphrase, and marks the configuration as provisioning with shared
/// placement. The bucket and its access roles are provisioned asynchronously;
/// any failure surfaces in the `last_init_error` field rather than in this
/// call's response. Unlike the `/backups/create` and `/backups/upsert`
/// endpoints (used for bring-your-own-account setups), the caller does not
/// supply a bucket or role ARNs, and no bucket probe is performed. Returns
/// 502 only if the server's secret storage isn't configured.
#[utoipa::path(
	post,
	path = "/create_shared",
	operation_id = "backups_create_shared",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = CreateSharedBackupConfigArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
		(status = 502, body = ProblemDetailsSchema),
	),
)]
pub async fn create_shared(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<CreateSharedBackupConfigArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;

	let kube = state.kube.as_ref().ok_or_else(|| {
		AppError::Upstream("secret store not configured; cannot create repo passphrase".into())
	})?;

	// Auto-name the bucket from the group name + a random suffix (whole name ≤ 63).
	let random = uuid::Uuid::new_v4().simple().to_string();
	let bucket = commons_servers::backup_jobs::shared_bucket_name(&group.name, &random[..8]);

	let passphrase = commons_servers::backup_secrets::generate_passphrase();
	let repo_password_ref = format!("backup-repo-{}", args.server_group_id);

	// The shared role ARNs + default region are the backups pod's concern (it has
	// `CANOPY_SHARED_BACKUP_*`): it stamps them into this row and provisions the
	// bucket at init. Private-server only marks the group `placement=shared` and
	// owns the passphrase Secret — so it needs no shared-account env. An
	// operator-supplied region is kept; otherwise the pod fills it.
	let config = ServerGroupBackupConfig::insert(
		&mut conn,
		NewServerGroupBackupConfig {
			group_id: args.server_group_id,
			bucket,
			prefix: String::new(),
			target_role_arn: String::new(),
			maintenance_role_arn: String::new(),
			region: args.region.clone(),
			repo_password_ref: repo_password_ref.clone(),
			status: BackupConfigStatus::Provisioning,
			mode: BackupRepoMode::FromBirth,
			placement: BackupPlacement::Shared,
		},
	)
	.await?;

	// Same all-or-nothing rollback as `create`: a failed Secret create must not
	// leave a half-created `provisioning` config with no passphrase.
	if let Err(e) = kube
		.create_password(&repo_password_ref, REPO_PASSWORD_SECRET_KEY, &passphrase)
		.await
	{
		let _ = ServerGroupBackupConfig::delete(&mut conn, args.server_group_id).await;
		return Err(e);
	}

	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Declaratively create or update a group's backup configuration.
///
/// Registers or converges a group's backup configuration in a single
/// idempotent call, generating and storing the repository passphrase and (for
/// a new configuration) provisioning the repository automatically. Creating a
/// configuration only succeeds onto an empty, unclaimed bucket — a bucket
/// that already holds a backup repository or other content, or one that
/// can't be accessed, is rejected. Re-applying an existing configuration
/// reconciles the role ARNs and region to match the request; `bucket` and
/// `prefix` are immutable and any mismatch is rejected.
#[utoipa::path(
	post,
	path = "/upsert",
	operation_id = "backups_upsert",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = UpsertBackupConfigArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 400, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
		(status = 502, body = ProblemDetailsSchema),
	),
)]
pub async fn upsert(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<UpsertBackupConfigArgs>,
) -> Result<Json<BackupConfigView>> {
	use crate::backup_probe::ProbeState;

	let mut conn = state.db.get().await?;
	ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;

	match ServerGroupBackupConfig::get(&mut conn, args.server_group_id).await? {
		// Update path: reconcile the mutable fields; bucket/prefix are immutable.
		Some(existing) => {
			// The pulumi config API is the BYO (`external`) path only. A
			// shared-account config is canopy-managed (its bucket + roles are
			// canopy's, not pulumi's) — refuse to reconcile it here so a stray
			// pulumi upsert can't overwrite the shared roles while leaving
			// `placement=shared` (an inconsistent row). Manage shared configs via
			// shared onboarding + delete/recreate instead.
			if existing.placement == BackupPlacement::Shared {
				return Err(AppError::Conflict(
					"this group is on shared-account backups (canopy-managed); manage it via shared-account onboarding (delete + recreate), not the pulumi config API"
						.into(),
				));
			}
			if existing.bucket != args.bucket || existing.prefix != args.prefix {
				return Err(AppError::Conflict(
					"bucket and prefix are immutable; delete the config and recreate it to change them"
						.into(),
				));
			}
			ServerGroupBackupConfig::update_roles_region(
				&mut conn,
				args.server_group_id,
				&args.target_role_arn,
				&args.maintenance_role_arn,
				args.region.as_deref(),
			)
			.await?;
			// Re-apply retries an incomplete/failed provision; a ready repo is
			// left untouched.
			let cfg = require_config(&mut conn, args.server_group_id).await?;
			if cfg.status != BackupConfigStatus::Ready {
				ServerGroupBackupConfig::mark_provisioning(&mut conn, args.server_group_id).await?;
			}
		}
		// Create path: from-birth onto an empty, unclaimed bucket only.
		None => {
			let probe = state
				.prober
				.probe(
					&args.bucket,
					&args.prefix,
					args.region.as_deref(),
					&args.maintenance_role_arn,
					Some(&args.target_role_arn),
				)
				.await?;
			match probe.state {
				ProbeState::Empty => {}
				ProbeState::KopiaRepo => {
					return Err(AppError::Conflict(
						"an existing kopia repository is here; import it with the interactive setup wizard, not the machine API"
							.into(),
					));
				}
				ProbeState::OtherContent => {
					return Err(AppError::Conflict(
						"bucket/prefix holds other (non-kopia) content; Canopy won't write over it"
							.into(),
					));
				}
				ProbeState::Inaccessible => {
					return Err(AppError::Upstream(format!(
						"cannot access the bucket: {}",
						probe.error.unwrap_or_else(|| "unknown error".into())
					)));
				}
			}
			if let Some(other) = ServerGroupBackupConfig::list(&mut conn)
				.await?
				.into_iter()
				.find(|c| c.bucket == args.bucket && c.prefix == args.prefix)
			{
				return Err(AppError::Conflict(format!(
					"bucket/prefix already configured for group {}",
					other.group_id
				)));
			}

			let kube = state.kube.as_ref().ok_or_else(|| {
				AppError::Upstream(
					"secret store not configured; cannot create repo passphrase".into(),
				)
			})?;
			let repo_password_ref = format!("backup-repo-{}", args.server_group_id);
			ServerGroupBackupConfig::insert(
				&mut conn,
				NewServerGroupBackupConfig {
					group_id: args.server_group_id,
					bucket: args.bucket.clone(),
					prefix: args.prefix.clone(),
					target_role_arn: args.target_role_arn.clone(),
					maintenance_role_arn: args.maintenance_role_arn.clone(),
					region: args.region.clone(),
					repo_password_ref: repo_password_ref.clone(),
					status: BackupConfigStatus::Provisioning,
					mode: BackupRepoMode::FromBirth,
					placement: BackupPlacement::External,
				},
			)
			.await?;
			kube.create_password(
				&repo_password_ref,
				REPO_PASSWORD_SECRET_KEY,
				&commons_servers::backup_secrets::generate_passphrase(),
			)
			.await?;
		}
	}

	let config = require_config(&mut conn, args.server_group_id).await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Request to inspect a bucket/prefix before configuring backups on it.
#[derive(Deserialize, ToSchema)]
pub struct ProbeArgs {
	/// Name of the S3 bucket to inspect.
	pub bucket: String,
	/// Key prefix within the bucket to inspect; defaults to empty (bucket
	/// root).
	#[serde(default)]
	pub prefix: String,
	/// AWS region the bucket is in, if known.
	pub region: Option<String>,
	/// AWS IAM role ARN to assume for the inspection (requires full read
	/// access).
	pub maintenance_role_arn: String,
	/// AWS IAM role ARN to additionally validate for device use (checked in
	/// both directions with a read-only no-op). Optional; supplying it lets
	/// the setup wizard catch a role configuration problem before saving,
	/// rather than when a device first attempts a backup.
	#[serde(default)]
	pub target_role_arn: Option<String>,
}

/// Result of inspecting a bucket/prefix: what's currently stored there, and
/// whether Canopy already has a configuration for it.
#[derive(Serialize, ToSchema)]
pub struct ProbeResponse {
	/// What was found at the inspected location: empty, an existing backup
	/// repository, other (non-backup) content, or inaccessible.
	#[schema(value_type = String)]
	pub state: crate::backup_probe::ProbeState,
	/// Present when `state` is `inaccessible`: a description of the failure
	/// encountered while trying to access the bucket.
	pub error: Option<String>,
	/// A few example object keys found, when `state` is `other_content`.
	pub object_sample: Vec<String>,
	/// The server group already configured for this exact bucket and prefix,
	/// if any.
	pub already_configured: Option<Uuid>,
}

/// Inspect a bucket/prefix for the backup setup wizard.
///
/// Assumes the maintenance role and reports what's there: empty, an existing
/// backup repository, other (non-backup) content, or inaccessible — plus
/// whether Canopy already has a configuration for this bucket and prefix.
/// Read-only; it never creates or modifies anything.
#[utoipa::path(
	post,
	path = "/probe",
	operation_id = "backups_probe",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = ProbeArgs,
	responses((status = 200, body = ProbeResponse)),
)]
pub async fn probe(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ProbeArgs>,
) -> Result<Json<ProbeResponse>> {
	let result = state
		.prober
		.probe(
			&args.bucket,
			&args.prefix,
			args.region.as_deref(),
			&args.maintenance_role_arn,
			args.target_role_arn.as_deref(),
		)
		.await?;

	let mut conn = state.db.get().await?;
	let already_configured = ServerGroupBackupConfig::list(&mut conn)
		.await?
		.into_iter()
		.find(|c| c.bucket == args.bucket && c.prefix == args.prefix)
		.map(|c| c.group_id);

	Ok(Json(ProbeResponse {
		state: result.state,
		error: result.error,
		object_sample: result.object_sample,
		already_configured,
	}))
}

/// Update a group's mutable backup configuration fields.
///
/// Currently only the region can be changed. Structural fields — bucket,
/// roles, mode — can only be set when the configuration is created;
/// schedule and retention are managed through the `/backups/set_schedule`
/// endpoint. Returns the updated configuration.
#[utoipa::path(
	post,
	path = "/update",
	operation_id = "backups_update",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = UpdateBackupConfigArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<UpdateBackupConfigArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	require_config(&mut conn, args.server_group_id).await?;
	let config = ServerGroupBackupConfig::update_region(
		&mut conn,
		args.server_group_id,
		args.region.as_deref(),
	)
	.await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Set the schedule and retention override for one backup type of a group.
///
/// A null interval means manual-only backups (no schedule). A specified
/// retention policy is validated against the organization's retention floor
/// and rejected with 400 if it falls below it, unless `allow_below_floor` is
/// set.
#[utoipa::path(
	post,
	path = "/set_schedule",
	operation_id = "backups_set_schedule",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = SetScheduleArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 400, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn set_schedule(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SetScheduleArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	require_config(&mut conn, args.server_group_id).await?;
	if let Some(policy) = &args.retention {
		if !args.allow_below_floor {
			policy.validate_floor()?;
		}
	}
	ServerGroupBackupSchedule::upsert(
		&mut conn,
		NewServerGroupBackupSchedule {
			group_id: args.server_group_id,
			r#type: args.r#type,
			expected_interval: args.expected_interval,
			retention: args.retention.map(|r| r.to_json()),
			allow_below_floor: args.allow_below_floor,
		},
	)
	.await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

// ── Per-type schedule/retention: canopy-wide defaults + per-group overrides ───

/// Org-floor baseline retention, used when a type has neither an override nor a
/// canopy-wide default (writes are floor-validated, so this only fills display).
const FLOOR_RETENTION: RetentionPolicy = RetentionPolicy {
	keep_latest: 1,
	keep_daily: RetentionPolicy::FLOOR_DAILY,
	keep_weekly: RetentionPolicy::FLOOR_WEEKLY,
	keep_monthly: RetentionPolicy::FLOOR_MONTHLY,
	keep_annual: 0,
};

/// Identifies a server group's schedule override for a backup type.
#[derive(Deserialize, ToSchema)]
pub struct ClearScheduleArgs {
	/// The server group to update.
	pub server_group_id: Uuid,
	/// Backup type whose schedule override to remove.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
}

/// Remove a group's schedule and retention override for a backup type.
///
/// The type reverts to inheriting the canopy-wide default schedule and
/// retention. Returns the updated configuration.
#[utoipa::path(
	post,
	path = "/clear_schedule",
	operation_id = "backups_clear_schedule",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = ClearScheduleArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn clear_schedule(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ClearScheduleArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	require_config(&mut conn, args.server_group_id).await?;
	ServerGroupBackupSchedule::delete(&mut conn, args.server_group_id, &args.r#type).await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Effective schedule and retention for one backup type of a group, combining
/// any per-group override with the canopy-wide default.
#[derive(Serialize, ToSchema)]
pub struct GroupTypeScheduleView {
	/// Backup type this schedule and retention apply to.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Seconds between scheduled runs; null = manual-only (no scheduled interval).
	pub effective_interval: Option<i64>,
	/// Retention policy that currently applies: the group's override if set,
	/// else the canopy-wide default for this type, else the organization's
	/// minimum retention floor.
	pub effective_retention: RetentionPolicy,
	/// Whether this group has an explicit override for this type, rather
	/// than inheriting the canopy-wide default.
	pub has_override: bool,
	/// Whether the effective config opts out of the org retention floor — taken
	/// from the override if present, else the type default.
	pub allow_below_floor: bool,
	/// When the next scheduled backup of this type is expected: the group's most
	/// recent successful backup of the type plus the interval — or "now" if the
	/// type is scheduled but has never succeeded yet. Null for manual-only types.
	pub next_run_at: Option<Timestamp>,
}

/// List the effective schedule and retention for every backup type a group's
/// servers have declared support for (not just the ones currently enabled).
///
/// A type with no scheduled interval still appears, with a null
/// `effective_interval`, since a manually run backup of that type is still
/// retained under its own policy.
#[utoipa::path(
	post,
	path = "/group_schedules",
	operation_id = "backups_group_schedules",
	tag = "backups",
	security(("tailscale-user" = [])),
	request_body = BackupsGroupArgs,
	responses((status = 200, body = Vec<GroupTypeScheduleView>)),
)]
pub async fn group_schedules(
	State(state): State<AppState>,
	Json(args): Json<BackupsGroupArgs>,
) -> Result<Json<Vec<GroupTypeScheduleView>>> {
	let mut conn = state.db.get().await?;
	let types =
		ServerBackupCapability::declared_types_for_group(&mut conn, args.server_group_id).await?;

	// Anchor for the next-expected-run estimate: the group's most recent
	// successful backup per type (max over its servers).
	let mut last_success: std::collections::HashMap<BackupType, Timestamp> =
		std::collections::HashMap::new();
	for ((_, ty), run) in
		BackupRun::latest_success_by_server_type_for_group(&mut conn, args.server_group_id).await?
	{
		last_success
			.entry(ty)
			.and_modify(|t| {
				if run.reported_at > *t {
					*t = run.reported_at;
				}
			})
			.or_insert(run.reported_at);
	}
	let now = Timestamp::now();

	let mut out = Vec::with_capacity(types.len());
	for ty in types {
		let over = ServerGroupBackupSchedule::get(&mut conn, args.server_group_id, &ty).await?;
		let def = BackupTypeDefault::get(&mut conn, &ty).await?;
		let effective_interval = over
			.as_ref()
			.and_then(|s| s.expected_interval)
			.or_else(|| def.as_ref().and_then(|d| d.default_interval))
			.map(|pg| pg.0.as_secs());
		let effective_retention = over
			.as_ref()
			.and_then(|s| s.retention.as_ref())
			.and_then(RetentionPolicy::from_json)
			.or_else(|| {
				def.as_ref()
					.and_then(|d| RetentionPolicy::from_json(&d.default_retention))
			})
			.unwrap_or(FLOOR_RETENTION);
		// Mirror the retention precedence: the override's flag governs when it
		// supplies the effective retention, else the type default's.
		let allow_below_floor = over
			.as_ref()
			.filter(|s| s.retention.is_some())
			.map(|s| s.allow_below_floor)
			.or_else(|| def.as_ref().map(|d| d.allow_below_floor))
			.unwrap_or(false);
		// Scheduled types: latest success + interval (or now if never run yet).
		// Manual-only types (no interval) have no expected next run.
		let next_run_at = effective_interval.map(|secs| {
			last_success
				.get(&ty)
				.and_then(|last| Timestamp::from_second(last.as_second() + secs).ok())
				.unwrap_or(now)
		});
		out.push(GroupTypeScheduleView {
			r#type: ty,
			effective_interval,
			effective_retention,
			allow_below_floor,
			has_override: over.is_some(),
			next_run_at,
		});
	}
	Ok(Json(out))
}

/// Canopy-wide default schedule and retention for a backup type.
#[derive(Serialize, ToSchema)]
pub struct TypeDefaultView {
	/// Backup type these defaults apply to.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Seconds between scheduled runs; null = manual-only.
	pub default_interval: Option<i64>,
	/// Default retention policy for this type, if set.
	pub default_retention: Option<RetentionPolicy>,
	/// Whether a server's capability for this type is enabled by default
	/// when first advertised.
	pub auto_enable: bool,
	/// Whether this default opts out of the org retention floor (dangerous).
	pub allow_below_floor: bool,
}

/// List the canopy-wide default schedule and retention for every backup type.
///
/// Groups inherit these defaults unless they set a per-group override.
#[utoipa::path(
	post,
	path = "/type_defaults",
	operation_id = "backups_type_defaults",
	tag = "backups",
	security(("tailscale-user" = [])),
	responses((status = 200, body = Vec<TypeDefaultView>)),
)]
pub async fn type_defaults(
	State(state): State<AppState>,
	_body: Json<serde_json::Value>,
) -> Result<Json<Vec<TypeDefaultView>>> {
	let mut conn = state.db.get().await?;
	let rows = BackupTypeDefault::list(&mut conn).await?;
	Ok(Json(
		rows.into_iter()
			.map(|d| TypeDefaultView {
				r#type: d.r#type,
				default_interval: d.default_interval.map(|pg| pg.0.as_secs()),
				default_retention: RetentionPolicy::from_json(&d.default_retention),
				auto_enable: d.auto_enable,
				allow_below_floor: d.allow_below_floor,
			})
			.collect(),
	))
}

/// Request to set the canopy-wide default schedule and retention for a
/// backup type.
#[derive(Deserialize, ToSchema)]
pub struct SetTypeDefaultArgs {
	/// Backup type these defaults apply to.
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Seconds between scheduled runs; null = manual-only.
	#[schema(value_type = Option<i64>, format = "int64")]
	pub default_interval: Option<PgDuration>,
	/// Default retention policy for this type.
	pub default_retention: RetentionPolicy,
	/// Whether a server's capability for this type should be enabled by
	/// default when first advertised.
	#[serde(default)]
	pub auto_enable: bool,
	/// Allows this default to specify retention below the organization's
	/// minimum retention floor. Defaults to false.
	#[serde(default)]
	pub allow_below_floor: bool,
}

/// Set the canopy-wide default schedule and retention for a backup type.
///
/// A retention policy below the organization's retention floor is rejected
/// with 400, unless `allow_below_floor` is set.
#[utoipa::path(
	post,
	path = "/set_type_default",
	operation_id = "backups_set_type_default",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = SetTypeDefaultArgs,
	responses((status = 200), (status = 400, body = ProblemDetailsSchema)),
)]
pub async fn set_type_default(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SetTypeDefaultArgs>,
) -> Result<Json<()>> {
	if !args.allow_below_floor {
		args.default_retention.validate_floor()?;
	}
	let mut conn = state.db.get().await?;
	BackupTypeDefault::upsert(
		&mut conn,
		NewBackupTypeDefault {
			r#type: args.r#type,
			default_interval: args.default_interval,
			default_retention: args.default_retention.to_json(),
			auto_enable: args.auto_enable,
			allow_below_floor: args.allow_below_floor,
		},
	)
	.await?;
	Ok(Json(()))
}

/// Trigger (or retry) provisioning of a group's backup repository.
///
/// Sets the configuration's status to provisioning and clears any previous
/// error, so provisioning is retried asynchronously; a failure surfaces in
/// the `last_init_error` field rather than in this call's response.
/// Idempotent — safe to call again while provisioning is already in
/// progress. Returns 409 if the repository is already ready.
#[utoipa::path(
	post,
	path = "/create_repo",
	operation_id = "backups_create_repo",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = BackupsGroupArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn create_repo(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<BackupsGroupArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	if config.status == BackupConfigStatus::Ready {
		return Err(AppError::Conflict(
			"repo already ready; cannot re-provision".into(),
		));
	}
	let config =
		ServerGroupBackupConfig::mark_provisioning(&mut conn, args.server_group_id).await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Request a one-off backup or restore for a server.
///
/// Idempotent per server, type, and purpose — re-requesting refreshes the
/// existing pending request rather than creating a duplicate.
#[utoipa::path(
	post,
	path = "/request_now",
	operation_id = "backups_request_now",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = RequestArgs,
	responses((status = 200), (status = 404, body = ProblemDetailsSchema)),
)]
pub async fn request_now(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<RequestArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	BackupRequest::enqueue(
		&mut conn,
		args.server_id,
		&args.r#type,
		args.purpose,
		Some(&admin.login),
	)
	.await?;
	Ok(Json(()))
}

/// Cancel a pending one-off backup or restore request.
///
/// Removes the pending request matching the given server, type, and
/// purpose. Cancelling when nothing is pending succeeds without effect.
#[utoipa::path(
	post,
	path = "/cancel_request",
	operation_id = "backups_cancel_request",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = RequestArgs,
	responses((status = 200)),
)]
pub async fn cancel_request(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<RequestArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	BackupRequest::clear(&mut conn, args.server_id, &args.r#type, args.purpose).await?;
	Ok(Json(()))
}

/// The current restore-window state for a server.
///
/// Reports until when the server may mint restore credentials for itself, or
/// `null` if restores are not currently allowed (never opened, or expired).
#[utoipa::path(
	post,
	path = "/restore_window",
	operation_id = "backups_restore_window",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = ServerArgs,
	responses(
		(status = 200, body = RestoreWindowView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn restore_window(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerArgs>,
) -> Result<Json<RestoreWindowView>> {
	let mut conn = state.db.get().await?;
	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	Ok(Json(RestoreWindowView::of(&server)))
}

/// Allow this server to restore for the next 24 hours.
///
/// Opens the server's restore window so an operator can run an ad-hoc
/// `bestool canopy restore` on it: while the window is open the device can mint
/// read-only restore credentials for its group's backup repository. The window
/// auto-expires after 24 hours; calling again re-arms it from now. Returns the
/// new window. Restores are gated behind this deliberate opt-in because they
/// grant read access to the group's entire backup history.
#[utoipa::path(
	post,
	path = "/allow_restore",
	operation_id = "backups_allow_restore",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = ServerArgs,
	responses(
		(status = 200, body = RestoreWindowView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn allow_restore(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<ServerArgs>,
) -> Result<Json<RestoreWindowView>> {
	let mut conn = state.db.get().await?;
	let allowed_until =
		Server::allow_restore(&mut conn, args.server_id, Some(&admin.login)).await?;
	Ok(Json(RestoreWindowView {
		allowed_until: Some(allowed_until),
		allowed_by: Some(admin.login),
	}))
}

/// Stop allowing this server to restore, immediately.
///
/// Closes the server's restore window now, before its 24-hour expiry. Any
/// credentials already minted keep their (at most one hour) validity, but no
/// new restore credentials can be minted until an operator allows restores
/// again. Closing an already-closed window succeeds without effect.
#[utoipa::path(
	post,
	path = "/disallow_restore",
	operation_id = "backups_disallow_restore",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = ServerArgs,
	responses((status = 200)),
)]
pub async fn disallow_restore(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Server::disallow_restore(&mut conn, args.server_id).await?;
	Ok(Json(()))
}

/// Request an out-of-cycle full maintenance run for a group's backup
/// repository.
///
/// It runs on the scheduler's next pass rather than waiting for its regular
/// staggered interval. Idempotent — re-requesting keeps the request pending.
/// Requires the repository to be ready; returns 409 otherwise.
#[utoipa::path(
	post,
	path = "/request_maintenance",
	operation_id = "backups_request_maintenance",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = BackupsGroupArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn request_maintenance(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<BackupsGroupArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	if config.status != BackupConfigStatus::Ready {
		return Err(AppError::Conflict(
			"repo is not ready; maintenance can only run on a ready repo".into(),
		));
	}
	let config = ServerGroupBackupConfig::request_full_maintenance(
		&mut conn,
		args.server_group_id,
		Some(&admin.login),
	)
	.await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Cancel a pending full-maintenance request.
///
/// Clears the group's outstanding request for a one-off full maintenance
/// run. Cancelling when nothing is pending — or when the run has already
/// started — succeeds without effect. Returns the updated configuration.
#[utoipa::path(
	post,
	path = "/cancel_maintenance",
	operation_id = "backups_cancel_maintenance",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = BackupsGroupArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn cancel_maintenance(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<BackupsGroupArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	ServerGroupBackupConfig::clear_full_maintenance_request(&mut conn, config.group_id).await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Get backup statistics for a group.
///
/// Returns the group's cached repository stats, recent backup and
/// maintenance runs, pending one-off requests, and each member server's
/// declared backup capabilities.
#[utoipa::path(
	post,
	path = "/stats",
	operation_id = "backups_stats",
	tag = "backups",
	security(("tailscale-user" = [])),
	request_body = BackupsGroupArgs,
	responses(
		(status = 200, body = BackupStatsView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn stats(
	State(state): State<AppState>,
	Json(args): Json<BackupsGroupArgs>,
) -> Result<Json<BackupStatsView>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;

	let stats = BackupRepoStats::get(&mut conn, args.server_group_id).await?;
	let recent_runs =
		BackupRun::list_for_group(&mut conn, args.server_group_id, RECENT_LIMIT).await?;
	let recent_maintenance =
		BackupMaintenanceRun::list_for_group(&mut conn, args.server_group_id, RECENT_LIMIT).await?;
	let (s3_month_sent_bytes, s3_month_received_bytes) =
		BackupRun::s3_traffic_this_month_for_group(&mut conn, args.server_group_id).await?;

	// Pending requests + declared capabilities across the group's member servers.
	let members = group.list_servers(&mut conn).await?;
	// Latest successful backup per (server, type), to decorate each capability.
	let latest_by =
		BackupRun::latest_success_by_server_type_for_group(&mut conn, args.server_group_id).await?;
	// In-flight detection: latest backup-cred issuance per (device, type) and the
	// latest reported run (any outcome) per (server, type).
	let latest_issuance = BackupCredentialIssuance::latest_backup_by_device_type_for_group(
		&mut conn,
		args.server_group_id,
	)
	.await?;
	let latest_report =
		BackupRun::latest_report_by_server_type_for_group(&mut conn, args.server_group_id).await?;
	let device_by_server: std::collections::HashMap<Uuid, Option<Uuid>> =
		members.iter().map(|s| (s.id, s.device_id)).collect();
	let now = Timestamp::now();
	let mut intervals: std::collections::HashMap<BackupType, Option<i64>> =
		std::collections::HashMap::new();
	let mut pending_requests = Vec::new();
	let mut capabilities = Vec::new();
	let mut restore_windows = Vec::new();
	for server in &members {
		if server.restore_allowed() {
			if let Some(allowed_until) = server.restore_allowed_until {
				restore_windows.push(RestoreWindowRow {
					server_id: server.id,
					allowed_until,
					allowed_by: server.restore_allowed_by.clone(),
				});
			}
		}
		for req in BackupRequest::pending_for_server(&mut conn, server.id).await? {
			pending_requests.push(PendingRequestRow {
				server_id: req.server_id,
				r#type: req.r#type,
				purpose: req.purpose,
				requested_at: req.requested_at,
				requested_by: req.requested_by,
			});
		}
		for cap in ServerBackupCapability::list_for_server(&mut conn, server.id).await? {
			let last = latest_by.get(&(cap.server_id, cap.r#type.clone()));
			let interval = match intervals.get(&cap.r#type) {
				Some(v) => *v,
				None => {
					let v = effective_interval_secs(&mut conn, args.server_group_id, &cap.r#type)
						.await?;
					intervals.insert(cap.r#type.clone(), v);
					v
				}
			};
			let issuance = device_by_server
				.get(&cap.server_id)
				.copied()
				.flatten()
				.and_then(|d| latest_issuance.get(&(d, cap.r#type.clone())).copied());
			let last_report = latest_report
				.get(&(cap.server_id, cap.r#type.clone()))
				.copied();
			capabilities.push(ServerBackupCapabilityView {
				server_id: cap.server_id,
				latest_snapshot_id: last.and_then(|r| r.snapshot_id.clone()),
				latest_snapshot_at: last.map(|r| r.reported_at),
				latest_snapshot_bytes: last.and_then(|r| r.bytes_uploaded),
				next_backup_at: next_backup_at(
					cap.enabled,
					interval,
					last.map(|r| r.reported_at),
					now,
				),
				processing_since: processing_since(now, issuance, last_report),
				r#type: cap.r#type,
				enabled: cap.enabled,
			});
		}
	}

	Ok(Json(BackupStatsView {
		stats,
		recent_runs,
		recent_maintenance,
		pending_requests,
		capabilities,
		restore_windows,
		s3_month_sent_bytes,
		s3_month_received_bytes,
	}))
}

/// List a server's backup capabilities and their enabled state.
///
/// Returns one entry per backup type the server has advertised, with its
/// enabled state, scheduling information, and recent run status. Empty if
/// the server hasn't advertised any capabilities yet.
#[utoipa::path(
	post,
	path = "/capabilities",
	operation_id = "backups_capabilities",
	tag = "backups",
	security(("tailscale-user" = [])),
	request_body = ServerArgs,
	responses((status = 200, body = Vec<ServerBackupCapabilityView>)),
)]
pub async fn capabilities(
	State(state): State<AppState>,
	Json(args): Json<ServerArgs>,
) -> Result<Json<Vec<ServerBackupCapabilityView>>> {
	let mut conn = state.db.get().await?;
	let (group_id, device_id) = Server::get_by_id(&mut conn, args.server_id)
		.await
		.ok()
		.map(|s| (s.group_id, s.device_id))
		.unwrap_or((None, None));
	let now = Timestamp::now();
	// Reuse the group-wide in-flight maps, then look up this server/device.
	let (latest_issuance, latest_report) = match group_id {
		Some(g) => (
			BackupCredentialIssuance::latest_backup_by_device_type_for_group(&mut conn, g).await?,
			BackupRun::latest_report_by_server_type_for_group(&mut conn, g).await?,
		),
		None => Default::default(),
	};
	let rows = ServerBackupCapability::list_for_server(&mut conn, args.server_id).await?;
	let mut out = Vec::with_capacity(rows.len());
	for c in rows {
		let last = BackupRun::latest_success_for_server(&mut conn, c.server_id, &c.r#type).await?;
		let interval = match group_id {
			Some(g) => effective_interval_secs(&mut conn, g, &c.r#type).await?,
			None => None,
		};
		let issuance = device_id.and_then(|d| latest_issuance.get(&(d, c.r#type.clone())).copied());
		let last_report = latest_report.get(&(c.server_id, c.r#type.clone())).copied();
		out.push(ServerBackupCapabilityView {
			server_id: c.server_id,
			latest_snapshot_id: last.as_ref().and_then(|r| r.snapshot_id.clone()),
			latest_snapshot_at: last.as_ref().map(|r| r.reported_at),
			latest_snapshot_bytes: last.as_ref().and_then(|r| r.bytes_uploaded),
			next_backup_at: next_backup_at(
				c.enabled,
				interval,
				last.as_ref().map(|r| r.reported_at),
				now,
			),
			processing_since: processing_since(now, issuance, last_report),
			r#type: c.r#type,
			enabled: c.enabled,
		});
	}
	Ok(Json(out))
}

/// Enable or disable a server's backup capability for one type.
///
/// Disabled capabilities are excluded from scheduling and can't be issued
/// backup credentials, outside of explicit operator-requested runs.
#[utoipa::path(
	post,
	path = "/set_capability",
	operation_id = "backups_set_capability",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = SetCapabilityArgs,
	responses((status = 200), (status = 404, body = ProblemDetailsSchema)),
)]
pub async fn set_capability(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SetCapabilityArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	ServerBackupCapability::set_enabled(&mut conn, args.server_id, &args.r#type, args.enabled)
		.await?;
	Ok(Json(()))
}

/// Delete a group's backup configuration (decommission).
///
/// The bucket and its existing backups are left untouched — object lock
/// prevents deletion — but this stops credential issuance for the group and
/// deletes the stored repository passphrase, which must not outlive its
/// configuration.
#[utoipa::path(
	post,
	path = "/delete",
	operation_id = "backups_delete",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = BackupsGroupArgs,
	responses((status = 200), (status = 404, body = ProblemDetailsSchema)),
)]
pub async fn delete(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<BackupsGroupArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	ServerGroupBackupConfig::delete(&mut conn, args.server_group_id).await?;
	// The Canopy-owned passphrase Secret should not outlive its config.
	if let Some(kube) = state.kube.as_ref() {
		kube.delete_password(&config.repo_password_ref).await?;
	}
	Ok(Json(()))
}

// ── recovery vault verification ceremony ───────────────────────────────────────────

/// Status of the disaster-recovery verification ceremony: whether recovery
/// is configured, and whether a fresh verification is due.
#[derive(Serialize, ToSchema)]
pub struct RecoveryStatusResponse {
	/// Whether recovery recipients are configured on this server at all.
	pub configured: bool,
	/// Fingerprints of the currently configured recovery recipients (age
	/// public keys).
	pub recipients: Vec<String>,
	/// When the recovery ceremony was last completed successfully, if ever.
	pub last_verified_at: Option<Timestamp>,
	/// The recipient set the last verification covered.
	pub last_verified_recipients: Vec<String>,
	/// Whether a fresh verification ceremony is due.
	pub due: bool,
	/// Human-readable reason for the `due` value.
	pub reason: String,
	/// When the vault object was last successfully written by the backups pod.
	pub last_write_at: Option<Timestamp>,
	/// Size (bytes) of the ciphertext from the last successful write.
	pub last_write_bytes: Option<i64>,
}

/// A freshly issued recovery-verification challenge.
#[derive(Serialize, ToSchema)]
pub struct RecoveryChallengeResponse {
	/// A single-use verification challenge encrypted to the recovery
	/// recipients, base64-encoded. The operator decrypts this offline with a
	/// held private key (any tool that supports the `age` encryption format)
	/// and submits the decrypted plaintext to the `/backups/recovery_verify`
	/// endpoint to prove the key is genuinely held.
	pub ciphertext_base64: String,
	/// The recovery recipients (age public keys) this challenge was
	/// encrypted to.
	pub recipients: Vec<String>,
}

/// Answer to an outstanding recovery-verification challenge.
#[derive(Deserialize, ToSchema)]
pub struct RecoveryVerifyArgs {
	/// The plaintext obtained by decrypting the challenge from the
	/// `/backups/recovery_challenge` endpoint.
	pub answer: String,
}

/// Result of a successful recovery verification.
#[derive(Serialize, ToSchema)]
pub struct RecoveryVerifyResponse {
	/// When the verification was recorded.
	pub verified_at: Timestamp,
}

/// Compute whether the ceremony is due: never run, last run > 1 year ago, or the
/// recipient set changed since the last verification.
fn recovery_due(
	latest: Option<&BackupRecoveryVerification>,
	current: &[String],
	now: Timestamp,
) -> (bool, String) {
	let Some(latest) = latest else {
		return (true, "never verified".into());
	};
	if now.as_second() - latest.verified_at.as_second() > RECOVERY_VERIFICATION_MAX_AGE_SECS {
		return (true, "last verification was over a year ago".into());
	}
	let mut prev = latest.recipient_list();
	prev.sort();
	let mut cur = current.to_vec();
	cur.sort();
	if prev != cur {
		return (
			true,
			"the recipient set changed since the last verification".into(),
		);
	}
	(
		false,
		"verified within the last year for the current recipients".into(),
	)
}

/// Get the disaster-recovery verification status.
///
/// Reports the configured recovery recipients, when recovery was last
/// verified, and whether a fresh verification is due (either because a
/// year has passed or because the recipient set changed).
#[utoipa::path(
	post,
	path = "/recovery_status",
	operation_id = "backups_recovery_status",
	tag = "backups",
	security(("tailscale-admin" = [])),
	responses((status = 200, body = RecoveryStatusResponse)),
)]
pub async fn recovery_status(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	_body: Json<serde_json::Value>,
) -> Result<Json<RecoveryStatusResponse>> {
	let recipients = state
		.recovery_recipients
		.as_ref()
		.map(|r| r.fingerprints().to_vec())
		.unwrap_or_default();
	let mut conn = state.db.get().await?;
	let latest = BackupRecoveryVerification::latest(&mut conn).await?;
	let (due, reason) = recovery_due(latest.as_ref(), &recipients, Timestamp::now());
	let last_write = RecoveryVaultWrite::latest(&mut conn).await?;
	Ok(Json(RecoveryStatusResponse {
		configured: state.recovery_recipients.is_some(),
		recipients,
		last_verified_at: latest.as_ref().map(|v| v.verified_at),
		last_verified_recipients: latest.map(|v| v.recipient_list()).unwrap_or_default(),
		due,
		reason,
		last_write_at: last_write.as_ref().map(|w| w.written_at),
		last_write_bytes: last_write.map(|w| w.bytes),
	}))
}

/// Issue a fresh recovery-verification challenge: a nonce encrypted to the
/// configured recovery recipients.
///
/// The operator decrypts it offline to prove a private key is genuinely
/// held, then submits the plaintext to the `/backups/recovery_verify`
/// endpoint. Returns 502 if recovery recipients aren't configured on this
/// server.
#[utoipa::path(
	post,
	path = "/recovery_challenge",
	operation_id = "backups_recovery_challenge",
	tag = "backups",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, body = RecoveryChallengeResponse),
		(status = 502, body = ProblemDetailsSchema),
	),
)]
pub async fn recovery_challenge(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	_body: Json<serde_json::Value>,
) -> Result<Json<RecoveryChallengeResponse>> {
	use base64::prelude::*;

	let recipients = state.recovery_recipients.as_ref().ok_or_else(|| {
		AppError::Upstream("recovery recipients are not configured on this server".into())
	})?;

	let nonce = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
	let ciphertext = recipients.encrypt(nonce.as_bytes())?;

	*state.recovery_challenge.lock().unwrap() = Some(RecoveryChallenge {
		nonce,
		issued_at: Timestamp::now(),
	});

	Ok(Json(RecoveryChallengeResponse {
		ciphertext_base64: BASE64_STANDARD.encode(ciphertext),
		recipients: recipients.fingerprints().to_vec(),
	}))
}

/// Complete the recovery-verification ceremony.
///
/// Checks that the submitted answer matches the outstanding challenge
/// issued by the `/backups/recovery_challenge` endpoint, then records the
/// verification against the current recipient set. Returns 400 if there's
/// no outstanding challenge, it has expired, or the answer is wrong.
#[utoipa::path(
	post,
	path = "/recovery_verify",
	operation_id = "backups_recovery_verify",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = RecoveryVerifyArgs,
	responses(
		(status = 200, body = RecoveryVerifyResponse),
		(status = 400, body = ProblemDetailsSchema),
		(status = 502, body = ProblemDetailsSchema),
	),
)]
pub async fn recovery_verify(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<RecoveryVerifyArgs>,
) -> Result<Json<RecoveryVerifyResponse>> {
	let recipients = state.recovery_recipients.as_ref().ok_or_else(|| {
		AppError::Upstream("recovery recipients are not configured on this server".into())
	})?;

	// Take (consume) the pending challenge: success or failure, it's single-use.
	let pending = state.recovery_challenge.lock().unwrap().take();
	let Some(pending) = pending else {
		return Err(AppError::BadRequest(
			"no outstanding challenge; request one first".into(),
		));
	};
	if Timestamp::now().as_second() - pending.issued_at.as_second() > RECOVERY_CHALLENGE_TTL_SECS {
		return Err(AppError::BadRequest(
			"challenge expired; request a fresh one".into(),
		));
	}
	if args.answer.trim() != pending.nonce {
		return Err(AppError::BadRequest(
			"answer does not match the challenge".into(),
		));
	}

	let mut conn = state.db.get().await?;
	let recorded = BackupRecoveryVerification::record(&mut conn, recipients.fingerprints()).await?;
	Ok(Json(RecoveryVerifyResponse {
		verified_at: recorded.verified_at,
	}))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn verification(verified_at_secs: i64, recipients: &[&str]) -> BackupRecoveryVerification {
		BackupRecoveryVerification {
			id: 1,
			verified_at: Timestamp::from_second(verified_at_secs).unwrap(),
			recipients: serde_json::json!(recipients),
		}
	}

	#[test]
	fn recovery_due_logic() {
		let now = Timestamp::from_second(2_000_000_000).unwrap();
		let recips = ["age1aaa".to_string(), "age1bbb".to_string()];

		// Never verified.
		assert!(recovery_due(None, &recips, now).0);

		// Fresh + same set (order-insensitive) → not due.
		let fresh = verification(now.as_second() - 10, &["age1bbb", "age1aaa"]);
		assert!(!recovery_due(Some(&fresh), &recips, now).0);

		// Over a year old → due.
		let old = verification(now.as_second() - (366 * 24 * 3600), &["age1aaa", "age1bbb"]);
		assert!(recovery_due(Some(&old), &recips, now).0);

		// Recent but recipient set changed → due.
		let changed = verification(now.as_second() - 10, &["age1aaa"]);
		assert!(recovery_due(Some(&changed), &recips, now).0);
	}
}

/// Fetch a group's config or 404 — used by the mutation handlers that require
/// an existing config row.
async fn require_config(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group_id: Uuid,
) -> Result<ServerGroupBackupConfig> {
	ServerGroupBackupConfig::get_required(conn, group_id).await
}
