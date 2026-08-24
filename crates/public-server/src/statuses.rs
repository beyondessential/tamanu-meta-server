use std::collections::BTreeMap;

use axum::{
	Json,
	extract::{Path, State},
};
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{
	backup_jobs::backups_due_now_for_server,
	device_auth::ServerDevice,
	headers::VersionHeader,
	server_tags::{effective_tags_for_server, tags_for_grading as status_tags_for_grading},
	status_ingest::{self, DEFAULT_SOURCE},
};
use commons_types::{
	backup::BackupType,
	device::DeviceRole,
	server::TagMap,
	status::{CheckResult, CheckSeverity},
};
use database::{
	Db, check_policies::CheckPolicy, devices::Device, diesel_async::AsyncPgConnection,
	servers::Server, silenced_refs::silenced_health_checks_for_server,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

/// A status push: a server's periodic heartbeat carrying its self-reported
/// health.
///
/// Besides the reserved `healthy` and `health` keys described here, any
/// additional top-level fields are accepted and stored verbatim as extra
/// status data.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StatusPayload {
	/// The name of the source pushing this status: the reporting agent, e.g.
	/// `alertd`. Multiple sources may report on one server, each with its own
	/// set of checks; a source's push only opens and recovers its own checks.
	///
	/// **Transitionally optional: this field will become mandatory.** A push
	/// without a `source` is attributed to `alertd`; new reporters must send
	/// their own name. Must be a non-empty string; the names `canopy` and
	/// `manual` are reserved for canopy itself and are rejected.
	pub source: Option<String>,

	/// Overall self-reported health of the server. **Absent means `true`**,
	/// so senders that predate this field are never treated as unhealthy by
	/// omission. Recorded for historical analysis and display, but **not
	/// consulted for incident or severity decisions** — those are derived
	/// from the per-check results in `health`, with each check's severity
	/// controlled by an operator-managed catalog.
	pub healthy: Option<bool>,

	/// Per-check breakdown. A push without a `health` array is the legacy
	/// Tamanu direct-report format: it is treated as the `tamanu` source
	/// reporting a single always-passing `tasks` heartbeat check. May be
	/// empty (`[]`) for a source that genuinely runs no checks — which
	/// recovers every check it previously reported. Each entry must
	/// include a non-empty `check` name and exactly one of `result` /
	/// `healthy`; any additional fields per check (latency, free disk %,
	/// certificate expiry, etc.) are passed through verbatim and shown in the
	/// status UI.
	///
	/// Every check name seen — whatever its result — is added to the
	/// operator-facing check catalog, where the policy grading its results
	/// can be reviewed and adjusted. A check whose effective result is
	/// failed or warning opens (or keeps open) its issue; a broken check
	/// keeps the same issue open, retaining a known failure's contribution
	/// while warning the check itself is broken; effective passed and
	/// skipped results open nothing and close prior issues.
	pub health: Vec<HealthCheck>,

	/// Free-form additional data (uptime, database version, timezone,
	/// hostname, etc.). Stored verbatim and surfaced as raw JSON in the
	/// status view.
	///
	/// A `tamanuVersion` field here is used as the server's tracked version
	/// (compared against the published version catalog), superseding the
	/// legacy `X-Version` request header. If both are present,
	/// `tamanuVersion` wins; if neither is, the status is recorded without a
	/// version.
	#[serde(flatten)]
	#[schema(additional_properties = true, value_type = Object)]
	pub extra: serde_json::Map<String, serde_json::Value>,
}

/// One health-check result within a status push.
#[derive(Debug, Deserialize, ToSchema)]
pub struct HealthCheck {
	/// Name of the check. Must be a non-empty string, and should stay stable
	/// across pushes: results for the same name are correlated over time, so
	/// successive failures and the eventual recovery land on the same issue.
	pub check: String,
	/// Outcome of the check: `passed`, `warning`, `failed`, `broken`, or
	/// `skipped`. Exactly one of `result` / `healthy` must be present per
	/// entry. `warning` and `failed` open the check's issue as graded by
	/// its policy; `broken` (the check itself errored, not the system under
	/// test) neither confirms nor clears a known failure — the issue stays
	/// open, retaining its contribution; `skipped` (a precondition was
	/// not met) and `passed` open nothing and close prior issues.
	pub result: Option<CheckResult>,
	/// Legacy pass/fail form: `true` means `passed`, `false` means `failed`.
	/// Mutually exclusive with `result`.
	pub healthy: Option<bool>,
	/// Arbitrary additional fields specific to this check (shown in the
	/// status UI as a key/value block, and available to operator-defined
	/// severity rules).
	#[serde(flatten)]
	#[schema(additional_properties = true, value_type = Object)]
	pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The status-push response: only the return-path instructions the device
/// can act on. The stored status record is deliberately not echoed back —
/// the device already has everything it sent.
#[derive(Debug, Serialize, ToSchema)]
pub struct StatusResponse {
	/// Backup types the server should back up now: operator-requested
	/// one-offs plus scheduled backups that are due. Each serializes as a
	/// plain string (e.g. `"tamanu-postgres"`). The device should run each
	/// listed type, then report via `POST /backup-report`; an empty list
	/// means nothing to do. Only sent to `alertd` pushes (the agent that
	/// runs backups); other sources always receive an empty list.
	#[schema(value_type = Vec<String>)]
	pub backup_now: Vec<BackupType>,
	/// The effective handling of every healthcheck canopy knows about, keyed
	/// by check name (as reported in `health[].check`): `skip` (silenced for
	/// this server, or classified below warning), `warn` (warning), or `fail`
	/// (error or critical). Only the static severity baseline is reflected —
	/// operator-defined conditional rules are evaluated per push and not
	/// included. Checks absent from the map are new to canopy and default to
	/// `warn`. Clients that predate this field can safely ignore it; the
	/// same mapping is served on demand at `GET /status/{server_id}/check-severities`.
	pub check_severities: BTreeMap<String, CheckSeverity>,
	/// What this server is entitled to do with names: the domains its group
	/// controls, the grants it holds, whether it is paused, and the names and
	/// certificates it already has. A server-wide fact, so returned to every
	/// source — an agent already reporting status learns of a new domain or a
	/// newly granted permission without asking separately. Identical to what
	/// `GET /names/entitlements` returns. Clients that predate this field can
	/// safely ignore it.
	// spec: CRT#what-a-server-may-act-on
	pub names: crate::names::Entitlements,
	/// The server's effective tags: its own tags overlaid on its group's,
	/// plus the synthetic read-only `canopy:` tags and effective `billing.*`
	/// labels. Identical to what the standalone `GET /tags` endpoint
	/// returns — see that endpoint for the full contract. Clients that
	/// predate this field can safely ignore it.
	pub tags: TagMap,
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(create))
		.routes(routes!(check_severities))
}

/// Submit a status heartbeat for a server.
///
/// Records a periodic status push against the server identified in the
/// path: overall self-reported health, a per-check breakdown, and any
/// free-form extra data. Each failed or warning check opens (or keeps
/// open) an issue at that check's operator-configured severity, and each
/// passed check closes any issue it previously opened; the server's
/// tracked software version is also updated from the payload.
///
/// The calling device must be the one enrolled for this exact server (or
/// hold the admin role). The response carries only return-path
/// instructions: a `backup_now` list of backup types the server should
/// back up immediately — devices should treat a non-empty list as a
/// prompt to run those backups and report them afterwards — a
/// `check_severities` map describing how canopy classifies each known
/// healthcheck for this server (`skip`/`warn`/`fail`), and the server's
/// effective `tags` (as served by `GET /tags`). The stored status record
/// is not echoed back.
#[utoipa::path(
	post,
	path = "/{server_id}",
	operation_id = "submit_status",
	tag = "statuses",
	security(("server-device" = [])),
	params(
		("server_id" = Uuid, Path),
	),
	request_body(
		content = StatusPayload,
		description = "Status push. A body without a `health` array is the legacy Tamanu direct-report format, treated as the `tamanu` source reporting a single always-passing `tasks` heartbeat check.",
	),
	responses(
		(status = 200, body = StatusResponse),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
async fn create(
	Path(server_id): Path<Uuid>,
	State(db): State<Db>,
	State(dns_zones): State<Vec<commons_types::dns::ManagedZone>>,
	device: ServerDevice,
	current_version: Option<VersionHeader>,
	body: Option<Json<serde_json::Value>>,
) -> Result<Json<StatusResponse>> {
	let mut db = db.get().await?;
	let Device { role, id, .. } = device.0.0;

	let server = Server::get_by_id(&mut db, server_id).await?;
	let is_authorized = role == DeviceRole::Admin || server.device_id == Some(id);

	if !is_authorized {
		return Err(AppError::custom(
			"device is not authorized to create statuses",
		));
	}

	let raw = body.map(|j| j.0).unwrap_or(serde_json::Value::Null);
	let parsed = status_ingest::parse_push(raw, current_version.map(|v| v.0))?;
	let source = parsed.source.clone();

	// The server's effective tags: stored server+group tags plus the
	// synthetic `canopy:*` tags and computed `billing.*` labels. Computed
	// once and used for both grading (per CHK, rules evaluate against the
	// effective tags, so a rule can predicate on any of them) and the
	// device response. JSON-wrapped so the rule evaluator compares
	// uniformly with extras.
	let effective_tags = effective_tags_for_server(&mut db, &server).await?;
	let tags = status_tags_for_grading(&effective_tags);

	// Recording is conditional on the source's ingest mode; everything else —
	// backup instructions, tags, severities computed below — is returned
	// regardless, so an ignored reporter keeps working. A denied source is
	// rejected from inside the ingestion core.
	status_ingest::ingest_push(&mut db, &server, id, &parsed, &tags).await?;

	// Tell the device which backup types to run now (operator one-offs +
	// schedule-due), riding the heartbeat response. Only alertd runs
	// backups — other sources (the tamanu heartbeat, seedling) would
	// treat an instruction they can't act on as noise at best. Empty for
	// an ungrouped server or one whose group has no `ready` backup config.
	let backup_now = match server.group_id {
		Some(group_id) if source == DEFAULT_SOURCE => {
			backups_due_now_for_server(&mut db, server_id, group_id, Timestamp::now()).await?
		}
		_ => Vec::new(),
	};

	// Computed after the transaction so checks first seen on this very push
	// (upserted into the catalog above) are already in the map.
	let check_severities =
		effective_check_severities(&mut db, server_id, server.group_id, &source).await?;

	// A server-wide fact, like the tags: every source gets it, so an agent
	// reporting status learns of a new domain or grant without a second call.
	// spec: CRT#what-a-server-may-act-on
	let names = crate::names::entitlements_for(&mut db, &server, &dns_zones).await?;

	Ok(Json(StatusResponse {
		backup_now,
		check_severities,
		names,
		tags: effective_tags,
	}))
}

/// Fetch the effective healthcheck severity mapping for a server.
///
/// Returns, for every healthcheck the `alertd` source reports, how that
/// check is handled for this server: `skip` (the check is silenced for
/// this server — at server or group scope — or its policy ceiling means it
/// never alerts), `warn` (graded at most a warning), or `fail` (failures
/// count as failures). Keys are check names as reported in
/// `health[].check` on status pushes. Only the static policy ceiling is
/// reflected; operator-defined conditional rules are evaluated per push
/// and not included here. The same mapping also rides along every
/// status-push response as `check_severities`, scoped to the pushing
/// source.
///
/// The calling device must be the one enrolled for this exact server (or
/// hold the admin role).
#[utoipa::path(
	get,
	path = "/{server_id}/check-severities",
	operation_id = "check_severities",
	tag = "statuses",
	security(("server-device" = [])),
	params(
		("server_id" = Uuid, Path),
	),
	responses(
		(status = 200, description = "Effective handling for each known check, keyed by check name.", body = BTreeMap<String, CheckSeverity>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
async fn check_severities(
	Path(server_id): Path<Uuid>,
	State(db): State<Db>,
	device: ServerDevice,
) -> Result<Json<BTreeMap<String, CheckSeverity>>> {
	let mut db = db.get().await?;
	let Device { role, id, .. } = device.0.0;

	let server = Server::get_by_id(&mut db, server_id).await?;
	if role != DeviceRole::Admin && server.device_id != Some(id) {
		return Err(AppError::custom(
			"device is not authorized to read this server's check severities",
		));
	}

	let map =
		effective_check_severities(&mut db, server_id, server.group_id, DEFAULT_SOURCE).await?;
	Ok(Json(map))
}

/// Build the effective per-check map for a server and source: every check
/// in the source's catalog mapped from its static policy ceiling (`failed`
/// → `fail`, `warning`/`broken` → `warn`, `passed`/`skipped` → `skip`),
/// then any check silenced for this server (at server or group scope)
/// forced to `skip`. Conditional rules are deliberately not consulted —
/// they depend on each push's contents, so only the static ceiling can be
/// mapped ahead of time.
async fn effective_check_severities(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Option<Uuid>,
	source: &str,
) -> Result<BTreeMap<String, CheckSeverity>> {
	let mut map: BTreeMap<String, CheckSeverity> = CheckPolicy::ceiling_map_for_source(db, source)
		.await?
		.into_iter()
		.map(|(name, ceiling)| (name, ceiling.into()))
		.collect();

	// Silences are keyed per (source, check): only this source's own
	// silences force its checks to skip.
	for check in silenced_health_checks_for_server(db, server_id, group_id, source).await? {
		map.insert(check, CheckSeverity::Skip);
	}

	Ok(map)
}
