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
	NewServerGroupBackupSchedule, RetentionPolicy, ServerBackupCapability, ServerGroupBackupConfig,
	ServerGroupBackupSchedule, server_groups::ServerGroup, servers::Server,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

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

/// Per-`(group,type)` schedule + retention override. `expected_interval` None =
/// manual-only (distinct from 0). `retention` None = inherit the type default.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ScheduleView {
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = Option<i64>, format = "int64")]
	pub expected_interval: Option<PgDuration>,
	pub retention: Option<RetentionPolicy>,
	/// Whether this override opts out of the org retention floor (dangerous).
	pub allow_below_floor: bool,
}

/// Full config + lifecycle for a group. Never includes the passphrase value.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackupConfigView {
	pub server_group_id: Uuid,
	pub bucket: String,
	pub prefix: String,
	pub target_role_arn: String,
	pub maintenance_role_arn: String,
	pub region: Option<String>,
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	#[schema(value_type = String)]
	pub status: BackupConfigStatus,
	/// `external` (BYO account) or `shared` (canopy-provisioned in the shared
	/// account). Lets the UI distinguish the two onboarding paths.
	#[schema(value_type = String)]
	pub placement: BackupPlacement,
	pub last_init_error: Option<String>,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
	/// When an operator has requested a one-off full maintenance run that the
	/// scheduler hasn't picked up yet; `None` = no pending request.
	pub force_full_maintenance_at: Option<Timestamp>,
	/// Who requested the pending full-maintenance run (Tailscale login).
	pub force_full_maintenance_by: Option<String>,
	/// Per-`(group,type)` schedule + retention overrides.
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

/// Fleet-overview row for the configured-groups listing.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackupConfigSummary {
	pub server_group_id: Uuid,
	pub bucket: String,
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	#[schema(value_type = String)]
	pub status: BackupConfigStatus,
	pub last_init_error: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct GroupArgs {
	pub server_group_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateBackupConfigArgs {
	pub server_group_id: Uuid,
	pub bucket: String,
	#[serde(default)]
	pub prefix: String,
	/// Device role: public-server assumes this to mint device creds (no delete).
	pub target_role_arn: String,
	/// Maintenance role: the backups pod assumes this for maintenance/inspection/
	/// s3-metrics (s3:* + delete + CloudWatch).
	pub maintenance_role_arn: String,
	pub region: Option<String>,
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	/// Passphrase mode only: the operator-supplied repo passphrase Canopy stores.
	/// From-birth ignores this (Canopy generates one).
	pub passphrase: Option<String>,
}

/// Machine-facing config-as-a-resource upsert (ops/pulumi). `mode` is implicit
/// — always from-birth — so the bucket must be empty and no passphrase is
/// supplied (importing an existing repo stays an interactive operator action).
/// `bucket`/`prefix` are the identity and immutable on re-apply; the role ARNs
/// and region are reconciled to the request each time. **Schedule and retention
/// are intentionally NOT part of this API** — they're per-`(group, type)` and
/// managed through the operator UI (inheriting the canopy-wide type defaults).
#[derive(Deserialize, ToSchema)]
pub struct UpsertBackupConfigArgs {
	pub server_group_id: Uuid,
	pub bucket: String,
	#[serde(default)]
	pub prefix: String,
	/// Device role: public-server assumes this to mint device creds (no delete).
	pub target_role_arn: String,
	/// Maintenance role: the backups pod assumes this for maintenance/inspection/
	/// s3-metrics (s3:* + delete + CloudWatch).
	pub maintenance_role_arn: String,
	pub region: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateBackupConfigArgs {
	pub server_group_id: Uuid,
	/// New region (or None to clear). Changing the region is allowed but is
	/// effectively a repo migration — the UI warns. Structural fields
	/// (bucket/role/mode) are not editable here.
	pub region: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct SetScheduleArgs {
	pub server_group_id: Uuid,
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Seconds; None = manual-only (no schedule), distinct from 0.
	#[schema(value_type = Option<i64>, format = "int64")]
	pub expected_interval: Option<PgDuration>,
	/// None = inherit the type default. A present policy is floor-validated
	/// unless `allow_below_floor` is set.
	pub retention: Option<RetentionPolicy>,
	/// Dangerous: opt this override out of the org retention floor, allowing a
	/// retention smaller than the org minimum. Defaults false.
	#[serde(default)]
	pub allow_below_floor: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct RequestArgs {
	pub server_id: Uuid,
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub purpose: BackupPurpose,
}

#[derive(Serialize, ToSchema)]
pub struct PendingRequestRow {
	pub server_id: Uuid,
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub purpose: BackupPurpose,
	pub requested_at: Timestamp,
	pub requested_by: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct BackupStatsView {
	pub stats: Option<BackupRepoStats>,
	pub recent_runs: Vec<BackupRun>,
	pub recent_maintenance: Vec<BackupMaintenanceRun>,
	pub pending_requests: Vec<PendingRequestRow>,
	/// Backup types each member server has advertised it can run (with their
	/// enabled state), so the "back up now" panel can offer the right types per
	/// server and grey out servers that have declared none.
	pub capabilities: Vec<ServerBackupCapabilityView>,
}

/// One `(server, type)` backup capability and whether the operator has it
/// enabled. `enabled` toggles whether the scheduler issues credentials and
/// schedules runs for the pair.
///
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

#[derive(Serialize, ToSchema)]
pub struct ServerBackupCapabilityView {
	pub server_id: Uuid,
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	pub enabled: bool,
	/// kopia snapshot id of this server+type's most recent successful backup,
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
	/// credentials were issued under an hour ago and no run has been reported
	/// since. `None` otherwise. Lets the UI show a "backing up…" state.
	pub processing_since: Option<Timestamp>,
}

#[derive(Deserialize, ToSchema)]
pub struct ServerArgs {
	pub server_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct SetCapabilityArgs {
	pub server_id: Uuid,
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	pub enabled: bool,
}

// ── Handlers ──────────────────────────────────────────────────────────────

/// Full config + lifecycle for a group. `null` (200) when the group has no
/// config (the zero-state); 404 only when the group itself doesn't exist.
#[utoipa::path(
	post,
	path = "/get",
	operation_id = "backups_get",
	tag = "backups",
	security(("tailscale-user" = [])),
	request_body = GroupArgs,
	responses(
		(status = 200, body = Option<BackupConfigView>),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get(
	State(state): State<AppState>,
	Json(args): Json<GroupArgs>,
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

/// All configured groups (fleet overview).
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

/// Insert a config row (`status='provisioning'`). Does NOT create the repo —
/// that's `create_repo`. 409 if the group already has a config; 404 if the
/// group is missing.
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

/// Args for [`create_shared`]: just the group (+ optional region override).
/// Canopy fills the bucket name and the shared role ARNs itself.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSharedBackupConfigArgs {
	pub server_group_id: Uuid,
	/// Region override; defaults to the shared-account default region.
	#[serde(default)]
	pub region: Option<String>,
}

/// Onboard a group onto **shared-account** backups — for deployments with no AWS
/// account of their own. Canopy auto-names a bucket
/// (`bes-canopy-backup-<group>-<random>`), generates + stores the passphrase, and
/// marks the config `provisioning`/`placement=shared` with **blank** role ARNs.
/// The backups pod stamps the shared device/maintenance role ARNs + region (from
/// its own `CANOPY_SHARED_BACKUP_*` env) and creates the bucket at init — so this
/// endpoint needs no shared-account env (a missing pod env surfaces as
/// `last_init_error`, not here). Unlike `create`/`upsert` (BYO), there's no
/// caller-supplied bucket/roles and no probe. 502 only if the secret store
/// (passphrase Secret) isn't configured.
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

/// Machine-facing config-as-a-resource upsert for ops/pulumi: declaratively
/// register/converge a group's backup config in one idempotent call (config +
/// generated passphrase Secret + schedule/retention + auto-provision). Creating
/// is from-birth onto an empty bucket only — a non-empty/existing-repo/
/// inaccessible bucket is rejected. Re-applying reconciles the role ARNs,
/// region, schedule and retention; `bucket`/`prefix` are immutable.
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

#[derive(Deserialize, ToSchema)]
pub struct ProbeArgs {
	pub bucket: String,
	#[serde(default)]
	pub prefix: String,
	pub region: Option<String>,
	/// Maintenance role to assume for the inspect (full read).
	pub maintenance_role_arn: String,
	/// Device/issuance role to also validate (assume both ways + read-only no-op).
	/// Optional; the wizard supplies it so a device-role trust gap is caught before
	/// saving rather than only when a device first backs up.
	#[serde(default)]
	pub target_role_arn: Option<String>,
}

/// Inspect-probe result for the wizard: what's at `bucket/prefix`, plus whether
/// Canopy already has a config for it.
#[derive(Serialize, ToSchema)]
pub struct ProbeResponse {
	#[schema(value_type = String)]
	pub state: crate::backup_probe::ProbeState,
	/// Present for `inaccessible`: the assume/list failure.
	pub error: Option<String>,
	/// A few keys, for the `other_content` warning.
	pub object_sample: Vec<String>,
	/// Group id if a config already exists for this exact bucket+prefix.
	pub already_configured: Option<Uuid>,
}

/// Synchronous setup-wizard probe: assume the maintenance role and inspect the
/// bucket/prefix (empty / kopia_repo / other_content / inaccessible), and report
/// whether Canopy already has a config for it. Read-only; never mutates.
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

/// Edit the non-structural config (region). Structural fields
/// (bucket/role/mode) are create-only; interval/retention live on
/// `set_schedule`.
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

/// Set (or clear) the per-`(group,type)` schedule + retention. None interval =
/// manual-only; a present retention is floor-validated (400 on violation).
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

#[derive(Deserialize, ToSchema)]
pub struct ClearScheduleArgs {
	pub server_group_id: Uuid,
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
}

/// Remove a per-`(group,type)` schedule override → revert that type to inheriting
/// the canopy-wide default.
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

/// Effective schedule/retention for one enabled backup type of a group: the
/// per-`(group,type)` override if present, else the canopy-wide default.
/// `has_override` tells the UI whether it's inheriting or overriding.
#[derive(Serialize, ToSchema)]
pub struct GroupTypeScheduleView {
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Seconds between scheduled runs; null = manual-only (no scheduled interval).
	pub effective_interval: Option<i64>,
	pub effective_retention: RetentionPolicy,
	pub has_override: bool,
	/// Whether the effective config opts out of the org retention floor — taken
	/// from the override if present, else the type default.
	pub allow_below_floor: bool,
	/// When the next scheduled backup of this type is expected: the group's most
	/// recent successful backup of the type plus the interval — or "now" if the
	/// type is scheduled but has never succeeded yet. Null for manual-only types.
	pub next_run_at: Option<Timestamp>,
}

/// Per **declared** backup type (not just enabled), the group's effective
/// schedule/retention (override or inherited default) — drives the per-type
/// editor in the group panel. Includes non-scheduled (disabled) types, since a
/// manual backup of one is still retained under its own type policy; those show
/// a null `effective_interval` ("manual only").
#[utoipa::path(
	post,
	path = "/group_schedules",
	operation_id = "backups_group_schedules",
	tag = "backups",
	security(("tailscale-user" = [])),
	request_body = GroupArgs,
	responses((status = 200, body = Vec<GroupTypeScheduleView>)),
)]
pub async fn group_schedules(
	State(state): State<AppState>,
	Json(args): Json<GroupArgs>,
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

/// Canopy-wide default schedule/retention for a backup type.
#[derive(Serialize, ToSchema)]
pub struct TypeDefaultView {
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Seconds between scheduled runs; null = manual-only.
	pub default_interval: Option<i64>,
	pub default_retention: Option<RetentionPolicy>,
	pub auto_enable: bool,
	/// Whether this default opts out of the org retention floor (dangerous).
	pub allow_below_floor: bool,
}

/// List the canopy-wide per-type defaults (the "global" schedule/retention each
/// group inherits unless it overrides a type).
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

#[derive(Deserialize, ToSchema)]
pub struct SetTypeDefaultArgs {
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Seconds between scheduled runs; null = manual-only.
	#[schema(value_type = Option<i64>, format = "int64")]
	pub default_interval: Option<PgDuration>,
	pub default_retention: RetentionPolicy,
	#[serde(default)]
	pub auto_enable: bool,
	/// Dangerous: opt this default out of the org retention floor. Defaults false.
	#[serde(default)]
	pub allow_below_floor: bool,
}

/// Set the canopy-wide default schedule/retention for a backup type
/// (floor-validated). Operators tune these in Settings → Backup defaults.
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

/// Record intent for the init Job: set/keep `provisioning`, clear
/// `last_init_error`. Idempotent retry. 409 if already `ready`.
#[utoipa::path(
	post,
	path = "/create_repo",
	operation_id = "backups_create_repo",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = GroupArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn create_repo(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<GroupArgs>,
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

/// One-off "backup now": upsert a `backup_requests` row keyed
/// `(server_id, type, purpose)`. Idempotent (re-request refreshes the row).
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

/// Cancel a pending one-off request.
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

/// Request a one-off full maintenance run for a group. The scheduler picks it up
/// on its next tick, bypassing the cadence jitter slot. Idempotent — re-request
/// refreshes the pending flag. Requires the repo to be `ready`.
#[utoipa::path(
	post,
	path = "/request_maintenance",
	operation_id = "backups_request_maintenance",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = GroupArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn request_maintenance(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<GroupArgs>,
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

/// Cancel a pending full-maintenance request the scheduler hasn't picked up yet.
/// A no-op if there's none pending (or the run already started).
#[utoipa::path(
	post,
	path = "/cancel_maintenance",
	operation_id = "backups_cancel_maintenance",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = GroupArgs,
	responses(
		(status = 200, body = BackupConfigView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn cancel_maintenance(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<GroupArgs>,
) -> Result<Json<BackupConfigView>> {
	let mut conn = state.db.get().await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	ServerGroupBackupConfig::clear_full_maintenance_request(&mut conn, config.group_id).await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
}

/// Stats panel: cached repo stats + recent runs + recent maintenance + pending
/// one-off requests (across the group's member servers).
#[utoipa::path(
	post,
	path = "/stats",
	operation_id = "backups_stats",
	tag = "backups",
	security(("tailscale-user" = [])),
	request_body = GroupArgs,
	responses(
		(status = 200, body = BackupStatsView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn stats(
	State(state): State<AppState>,
	Json(args): Json<GroupArgs>,
) -> Result<Json<BackupStatsView>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;

	let stats = BackupRepoStats::get(&mut conn, args.server_group_id).await?;
	let recent_runs =
		BackupRun::list_for_group(&mut conn, args.server_group_id, RECENT_LIMIT).await?;
	let recent_maintenance =
		BackupMaintenanceRun::list_for_group(&mut conn, args.server_group_id, RECENT_LIMIT).await?;

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
	for server in &members {
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
	}))
}

/// A server's registered backup capabilities + their enabled state. Empty when
/// the server has advertised none yet.
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

/// Operator toggle of a `(server, type)` capability's enabled flag.
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

/// Delete a group's config row (decommission). The bucket and its object-locked
/// objects persist; this only stops credential issuance and removes the
/// Canopy-owned passphrase Secret (which must not outlive its config).
#[utoipa::path(
	post,
	path = "/delete",
	operation_id = "backups_delete",
	tag = "backups",
	security(("tailscale-admin" = [])),
	request_body = GroupArgs,
	responses((status = 200), (status = 404, body = ProblemDetailsSchema)),
)]
pub async fn delete(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<GroupArgs>,
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

/// Status of the recovery vault verification ceremony.
#[derive(Serialize, ToSchema)]
pub struct RecoveryStatusResponse {
	/// Whether recovery recipients are configured on this server at all.
	pub configured: bool,
	/// The live recipient fingerprints (`age1…`).
	pub recipients: Vec<String>,
	pub last_verified_at: Option<Timestamp>,
	/// The recipient set the last verification covered.
	pub last_verified_recipients: Vec<String>,
	/// Whether a (fresh) ceremony is due.
	pub due: bool,
	/// Human-readable reason for the `due` value.
	pub reason: String,
}

#[derive(Serialize, ToSchema)]
pub struct RecoveryChallengeResponse {
	/// The challenge ciphertext (`age` to the recipients), base64-encoded. The
	/// operator decrypts it offline with a held private key (`bestool crypto
	/// decrypt` / `age`) and submits the plaintext to `recovery_verify`.
	pub ciphertext_base64: String,
	/// The recipients this challenge was encrypted to.
	pub recipients: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct RecoveryVerifyArgs {
	/// The decrypted challenge plaintext.
	pub answer: String,
}

#[derive(Serialize, ToSchema)]
pub struct RecoveryVerifyResponse {
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

/// Report the recovery vault verification status (recipients, last verification, and
/// whether a ceremony is due).
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
	Ok(Json(RecoveryStatusResponse {
		configured: state.recovery_recipients.is_some(),
		recipients,
		last_verified_at: latest.as_ref().map(|v| v.verified_at),
		last_verified_recipients: latest.map(|v| v.recipient_list()).unwrap_or_default(),
		due,
		reason,
	}))
}

/// Issue a verification challenge: a fresh nonce encrypted to the recovery recipients.
/// The operator decrypts it offline (proving a private key is genuinely held) and
/// posts the plaintext back to `recovery_verify`.
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

/// Complete the ceremony: verify the operator's decrypted answer matches the
/// outstanding challenge, then record the verification against the current
/// recipient set.
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
