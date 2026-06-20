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
	backup::{BackupConfigStatus, BackupPurpose, BackupRepoMode, BackupType},
};
use database::pg_duration::PgDuration;
use database::{
	BackupMaintenanceRun, BackupRepoStats, BackupRequest, BackupRun, NewServerGroupBackupConfig,
	NewServerGroupBackupSchedule, RetentionPolicy, ServerBackupCapability, ServerGroupBackupConfig,
	ServerGroupBackupSchedule, server_groups::ServerGroup,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

/// Secret key the from-birth init Job writes the generated passphrase under.
const REPO_PASSWORD_SECRET_KEY: &str = "password";

/// Cap on the recent-runs / maintenance lists in the stats panel.
const RECENT_LIMIT: i64 = 20;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(get))
		.routes(routes!(list))
		.routes(routes!(create))
		.routes(routes!(probe))
		.routes(routes!(update))
		.routes(routes!(set_schedule))
		.routes(routes!(create_repo))
		.routes(routes!(request_now))
		.routes(routes!(cancel_request))
		.routes(routes!(stats))
		.routes(routes!(capabilities))
		.routes(routes!(set_capability))
		.routes(routes!(delete))
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
	pub last_init_error: Option<String>,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
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
			last_init_error: config.last_init_error,
			created_at: config.created_at,
			updated_at: config.updated_at,
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
	/// None = inherit the type default. A present policy is floor-validated.
	pub retention: Option<RetentionPolicy>,
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
}

/// One `(server, type)` backup capability and whether the operator has it
/// enabled. `enabled` toggles whether the scheduler issues credentials and
/// schedules runs for the pair.
#[derive(Serialize, ToSchema)]
pub struct ServerBackupCapabilityView {
	pub server_id: Uuid,
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	pub enabled: bool,
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
		},
	)
	.await?;
	kube.create_password(&repo_password_ref, REPO_PASSWORD_SECRET_KEY, &passphrase)
		.await?;

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
		policy.validate_floor()?;
	}
	ServerGroupBackupSchedule::upsert(
		&mut conn,
		NewServerGroupBackupSchedule {
			group_id: args.server_group_id,
			r#type: args.r#type,
			expected_interval: args.expected_interval,
			retention: args.retention.map(|r| r.to_json()),
		},
	)
	.await?;
	let config = require_config(&mut conn, args.server_group_id).await?;
	Ok(Json(BackupConfigView::build(&mut conn, config).await?))
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

	// Pending requests across the group's member servers.
	let members = group.list_servers(&mut conn).await?;
	let mut pending_requests = Vec::new();
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
	}

	Ok(Json(BackupStatsView {
		stats,
		recent_runs,
		recent_maintenance,
		pending_requests,
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
	let rows = ServerBackupCapability::list_for_server(&mut conn, args.server_id).await?;
	Ok(Json(
		rows.into_iter()
			.map(|c| ServerBackupCapabilityView {
				server_id: c.server_id,
				r#type: c.r#type,
				enabled: c.enabled,
			})
			.collect(),
	))
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
/// objects persist; this only stops credential issuance.
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
	require_config(&mut conn, args.server_group_id).await?;
	ServerGroupBackupConfig::delete(&mut conn, args.server_group_id).await?;
	Ok(Json(()))
}

/// Fetch a group's config or 404 — used by the mutation handlers that require
/// an existing config row.
async fn require_config(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group_id: Uuid,
) -> Result<ServerGroupBackupConfig> {
	ServerGroupBackupConfig::get_required(conn, group_id).await
}
