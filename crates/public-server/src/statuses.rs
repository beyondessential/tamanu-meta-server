use std::collections::BTreeMap;

use axum::{
	Json,
	extract::{Path, State},
};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{
	backup_jobs::backups_due_now_for_server, device_auth::ServerDevice, headers::VersionHeader,
};
use commons_types::{backup::BackupType, device::DeviceRole, issue::Severity, status::CheckResult};
use database::{
	Db,
	devices::Device,
	diesel_async::{AsyncConnection, AsyncPgConnection},
	healthcheck_severities::{EvaluationContext, HealthcheckSeverity},
	issues::{Issue, NewEvent},
	servers::Server,
	statuses::{NewStatus, Status},
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::state::AppState;

/// Status payload contract — documents the wire shape only. The
/// handler actually receives `serde_json::Value` and runs its own
/// validation so arbitrary additional fields flow through into
/// `statuses.extra` unchanged.
///
/// Schema is reproduced here so the openapi spec describes the
/// `healthy` and `health` keys explicitly; otherwise consumers have
/// to read the prose to know they exist.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StatusPayload {
	/// Overall server self-reported health. **Absent ⇒ `true`** —
	/// legacy senders that don't know about this field never
	/// false-positive unhealthy. Persisted on `statuses.healthy` for
	/// historical analysis and the status snapshot UI, but **not
	/// consulted for incident or severity decisions**: canopy derives
	/// the system-healthy judgement from per-check results, with each
	/// check's severity coming from the operator-owned
	/// `healthcheck_severities` catalog.
	pub healthy: Option<bool>,

	/// Per-check breakdown. **Required** — a push without a `health`
	/// array is rejected with `400` (unless the server opts into the
	/// legacy format via `allow_legacy_status`, in which case such a push
	/// only refreshes reachability and carries prior checks forward). May
	/// be empty (`[]`) for a server that genuinely runs no checks. Each
	/// entry must include `check`
	/// (non-empty) and exactly one of `result` / `healthy`; arbitrary
	/// additional fields per check (latency, free disk %, certificate
	/// expiry, etc.) are passed through verbatim and surfaced in the
	/// status snapshot UI.
	///
	/// Each check name seen — whatever its result — upserts into
	/// `healthcheck_severities` so operators can review and adjust the
	/// severity assigned to its failures. Failed checks file (or keep
	/// active) an issue at `(status, health/<check>)` using the
	/// catalog's current severity for that check name; broken checks
	/// file at `(status, health-broken/<check>)` at a fixed Warning.
	pub health: Vec<HealthCheck>,

	/// Free-form additional data (uptime, postgres version, timezone,
	/// hostname, etc.). Stored verbatim in `statuses.extra` and
	/// surfaced as raw JSON in the status snapshot.
	#[serde(flatten)]
	#[schema(additional_properties = true, value_type = Object)]
	pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct HealthCheck {
	/// Identifier for the check. Must be a non-empty string. Used as
	/// the issue's `ref` (`health/<check>`) so the same check
	/// transitions land on the same issue across pushes.
	pub check: String,
	/// Outcome of the check. Exactly one of `result` / `healthy` must
	/// be present per entry. `failed` files an issue at the catalog
	/// severity; `broken` (the check itself errored, not the system
	/// under test) files at a fixed Warning; `skipped` (precondition
	/// not met) and `passed` file nothing and close prior issues.
	pub result: Option<CheckResult>,
	/// Legacy pass/fail for the check: `true` ⇒ `passed`, `false` ⇒
	/// `failed`. Mutually exclusive with `result`.
	pub healthy: Option<bool>,
	/// Arbitrary additional fields specific to the check (rendered in
	/// the snapshot UI as a key/value block).
	#[serde(flatten)]
	#[schema(additional_properties = true, value_type = Object)]
	pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `source` value for the events filed below. Distinct from
/// `canopy` (reachability sweep) so operators can tell apart "we
/// couldn't reach you" from "you told us you're sick".
const STATUS_SOURCE: &str = "status";
/// Prefix for per-check refs. Each failed check is filed at
/// `(status, health/<check_name>)`.
const HEALTH_REF: &str = "health";
/// Prefix for broken-check refs. A check reporting `result: broken`
/// files at `(status, health-broken/<check_name>)` — a separate ref
/// from the check's failure issue, so a known failure can stay open
/// (unconfirmed either way) while the check itself is broken, and so
/// operators can silence the two independently.
const BROKEN_REF: &str = "health-broken";

/// The status-push response: the persisted [`Status`] plus `backup_now`, the
/// backup types this server should back up *right now* (operator one-offs +
/// schedule-due; see [`backups_due_now_for_server`]). The device runs each; an
/// empty list means "nothing to do". The `Status` fields are flattened so they
/// stay top-level for clients that predate `backup_now` and ignore it.
#[derive(Debug, Serialize, ToSchema)]
pub struct StatusResponse {
	#[serde(flatten)]
	pub status: Status,
	/// Backup types due/requested for this server now. Each serializes as a
	/// plain string (e.g. `"tamanu-postgres"`).
	#[schema(value_type = Vec<String>)]
	pub backup_now: Vec<BackupType>,
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(create))
}

#[utoipa::path(
	post,
	path = "/{server_id}",
	tag = "statuses",
	security(("server-device" = [])),
	params(
		("server_id" = Uuid, Path),
	),
	request_body(
		content = StatusPayload,
		description = "Status push. A `health` array is required (it may be empty); a body without it is rejected with 400, unless the server has `allow_legacy_status` set, in which case the push refreshes reachability only and carries the last known healthchecks forward.",
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
	device: ServerDevice,
	current_version: VersionHeader,
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

	// Tell the device which backup types to run now (operator one-offs +
	// schedule-due), riding the heartbeat response. Empty for an ungrouped
	// server or one whose group has no `ready` backup config.
	let backup_now = match server.group_id {
		Some(group_id) => {
			backups_due_now_for_server(&mut db, server_id, group_id, Timestamp::now()).await?
		}
		None => Vec::new(),
	};

	let raw = body.map(|j| j.0).unwrap_or(serde_json::Value::Null);
	let (healthy, health, extra) = split_health_from_extra(raw)?;

	let Some(health) = health else {
		// Legacy format (no `health` array). Off by default — only servers an
		// operator has explicitly opted in via `allow_legacy_status` may use
		// it, until their reporter speaks the new format.
		if !server.allow_legacy_status {
			return Err(AppError::BadRequest("`health` array is required".into()));
		}
		let status = create_legacy_status(&mut db, server_id, id, extra, current_version.0).await?;
		return Ok(Json(StatusResponse { status, backup_now }));
	};

	// Resolve the server's effective tag map outside the write transaction.
	// Read-only; shared across every rule evaluation for this push. JSON-
	// wrapped so the rule evaluator can compare uniformly with extras.
	let tag_map = server.tags_merged_with_group(&mut db).await?;
	let tags: std::collections::HashMap<String, serde_json::Value> = tag_map
		.0
		.into_iter()
		.map(|(k, v)| (k, serde_json::Value::String(v)))
		.collect();

	// Insert + file events atomically. NewEvent::save itself opens
	// a transaction; diesel-async nests it as a SAVEPOINT.
	let status = db
		.transaction::<_, AppError, _>(async |conn| {
			let status = NewStatus {
				server_id,
				device_id: Some(id),
				version: Some(current_version.0),
				extra,
				healthy,
				health,
			}
			.save(conn)
			.await?;

			file_health_events(conn, server_id, Some(id), &status, &tags).await?;

			Ok(status)
		})
		.await?;

	Ok(Json(StatusResponse { status, backup_now }))
}

/// Store a legacy-format push (no `health` array) for a server that's opted
/// into [`Server::allow_legacy_status`]. The push only refreshes reachability:
/// the new row carries the server's last known `healthy`/`health` forward
/// (defaulting to "healthy, no checks" if the server has never reported the
/// new format) rather than wiping them, and no health events are filed. So a
/// server straddling an old and a new reporter doesn't flap its per-check
/// issues every time the legacy endpoint pings.
async fn create_legacy_status(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	device_id: Uuid,
	extra: serde_json::Value,
	version: commons_types::version::VersionStr,
) -> Result<Status> {
	let (healthy, health) = match Status::latest_for_server(db, server_id).await? {
		Some(prior) => (prior.healthy, prior.health),
		None => (true, serde_json::Value::Array(Vec::new())),
	};
	NewStatus {
		server_id,
		device_id: Some(device_id),
		version: Some(version),
		extra,
		healthy,
		health,
	}
	.save(db)
	.await
}

/// Per-push event filing. Warning/failed checks land at
/// `(status, health/<check>)`; recoveries close those issues. Broken
/// checks (`result: broken` — the check itself errored, not the
/// system under test) land at `(status, health-broken/<check>)` at a
/// fixed Warning; a broken check neither confirms nor clears a known
/// failure, so its `health/<check>` issue stays open. Skipped checks
/// (`result: skipped` — precondition not met) file nothing and close
/// both refs.
///
/// Severity for each warning/failed check comes from the
/// operator-owned `healthcheck_severities` catalog (see
/// [`HealthcheckSeverity::severity_for`] for the rules/fallback
/// contract). Every check seen on a push — whatever its result —
/// upserts a default catalog row so new checks are visible to
/// operators immediately at the default Warning severity.
/// `status.healthy` is intentionally not consulted: the catalog is
/// canopy's single source of truth for per-check severity.
async fn file_health_events(
	conn: &mut AsyncPgConnection,
	server_id: Uuid,
	device_id: Option<Uuid>,
	status: &Status,
	tags: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<()> {
	let curr_check_results = collect_check_results(&status.health);
	let occurred_at = Some(status.created_at);

	// Upsert a catalog row for every check name seen on this push,
	// whatever its result. New checks land at default Warning;
	// operators can review and adjust from the /healthchecks page.
	for check_name in curr_check_results.keys() {
		HealthcheckSeverity::upsert_default(conn, check_name).await?;
	}

	// Status-level extras are shared across every per-check evaluation.
	let empty_map = serde_json::Map::new();
	let status_extra = status.extra.as_object().unwrap_or(&empty_map);

	// Per-check opens: warning and failed file on the same ref (one
	// thread per check; the filed severity is what differs).
	for (check, result) in curr_check_results
		.iter()
		.filter(|(_, r)| matches!(r, CheckResult::Warning | CheckResult::Failed))
	{
		let entry = find_health_entry(&status.health, check);
		// Strip the reserved `check` / `healthy` keys, and replace any
		// wire-form `result` with the normalised value so rules see a
		// uniform `check.result` even for legacy (`healthy: bool`)
		// payloads.
		let mut check_extra = entry.cloned().unwrap_or_default();
		check_extra.remove("check");
		check_extra.remove("healthy");
		check_extra.insert(
			"result".into(),
			serde_json::Value::String(result.to_string()),
		);
		let ctx = EvaluationContext {
			status_extra,
			check_extra: &check_extra,
			tags,
		};
		let severity = HealthcheckSeverity::severity_for(conn, check, *result, &ctx).await?;
		let described = match result {
			CheckResult::Warning => "warned",
			_ => "failed",
		};
		NewEvent {
			source: STATUS_SOURCE.into(),
			r#ref: format!("{HEALTH_REF}/{check}"),
			severity: Some(severity),
			description: Some(format!("Health check '{check}' {described}")),
			message: per_check_description(entry).unwrap_or_default(),
			active: Some(true),
			occurred_at,
		}
		.save(conn, server_id, device_id)
		.await?;
	}

	// Broken opens: separate ref, fixed Warning. A broken check is a
	// monitoring blind spot — actionable (fix the check), but not the
	// system under test failing, so it doesn't inherit the catalog
	// severity and can't open incidents.
	for (check, _) in curr_check_results
		.iter()
		.filter(|(_, r)| matches!(r, CheckResult::Broken))
	{
		let entry = find_health_entry(&status.health, check);
		NewEvent {
			source: STATUS_SOURCE.into(),
			r#ref: format!("{BROKEN_REF}/{check}"),
			severity: Some(Severity::Warning),
			description: Some(format!("Health check '{check}' is broken")),
			message: per_check_description(entry).unwrap_or_default(),
			active: Some(true),
			occurred_at,
		}
		.save(conn, server_id, device_id)
		.await?;
	}

	// Per-check closes are derived from the issues that are actually
	// open, not from the previous status row: an issue can stay open
	// across pushes that don't re-file it (failed → broken keeps the
	// failure open), so the previous push alone can't tell us what
	// needs closing.

	// Failure closes: an open `health/<check>` closes when the check
	// is now passed, skipped, or unmentioned ("trust the reporter").
	// Broken does NOT close a prior failure — the check can't confirm
	// the failure either way while it's broken.
	let health_prefix = format!("{HEALTH_REF}/");
	for r#ref in
		Issue::active_refs_with_prefix(conn, server_id, STATUS_SOURCE, &health_prefix).await?
	{
		let Some(check) = r#ref.strip_prefix(&health_prefix) else {
			continue;
		};
		let curr = curr_check_results.get(check);
		if matches!(
			curr,
			Some(CheckResult::Warning | CheckResult::Failed | CheckResult::Broken)
		) {
			continue;
		}
		let message = if matches!(curr, Some(CheckResult::Skipped)) {
			format!("Health check '{check}' is now skipped")
		} else {
			format!("Health check '{check}' recovered")
		};
		NewEvent {
			source: STATUS_SOURCE.into(),
			r#ref,
			severity: Some(Severity::Info),
			description: None,
			message,
			active: Some(false),
			occurred_at,
		}
		.save(conn, server_id, device_id)
		.await?;
	}

	// Broken closes: any result other than broken (or absence) means
	// the check itself is no longer broken.
	let broken_prefix = format!("{BROKEN_REF}/");
	for r#ref in
		Issue::active_refs_with_prefix(conn, server_id, STATUS_SOURCE, &broken_prefix).await?
	{
		let Some(check) = r#ref.strip_prefix(&broken_prefix) else {
			continue;
		};
		if matches!(curr_check_results.get(check), Some(CheckResult::Broken)) {
			continue;
		}
		NewEvent {
			source: STATUS_SOURCE.into(),
			r#ref: format!("{BROKEN_REF}/{check}"),
			severity: Some(Severity::Info),
			description: None,
			message: format!("Health check '{check}' is no longer broken"),
			active: Some(false),
			occurred_at,
		}
		.save(conn, server_id, device_id)
		.await?;
	}

	Ok(())
}

/// Normalised result of every well-formed check in a `health[]` blob.
/// Anything malformed (non-object entry, missing/invalid `check`, no
/// resolvable result) is ignored — the public endpoint validates on
/// the way in, so by the time we read it back from the DB we're either
/// looking at our own well-formed data or at historical pre-contract
/// rows where missing means absent. Reads both the `result` enum form
/// and the legacy `healthy: bool` form via [`CheckResult::from_entry`].
fn collect_check_results(health: &serde_json::Value) -> BTreeMap<String, CheckResult> {
	let Some(arr) = health.as_array() else {
		return BTreeMap::new();
	};
	arr.iter()
		.filter_map(|e| {
			let obj = e.as_object()?;
			let check = obj.get("check")?.as_str()?;
			let result = CheckResult::from_entry(obj)?;
			Some((check.to_string(), result))
		})
		.collect()
}

fn find_health_entry<'a>(
	health: &'a serde_json::Value,
	name: &str,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
	health.as_array()?.iter().find_map(|e| {
		let obj = e.as_object()?;
		let check = obj.get("check")?.as_str()?;
		(check == name).then_some(obj)
	})
}

fn per_check_description(
	entry: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
	let entry = entry?;
	let mut lines = Vec::new();
	for (k, v) in entry.iter() {
		if k == "check" || k == "healthy" || k == "result" {
			continue;
		}
		let rendered = match v {
			serde_json::Value::String(s) => s.clone(),
			other => other.to_string(),
		};
		lines.push(format!("- **{k}**: `{rendered}`"));
	}
	(!lines.is_empty()).then(|| lines.join("\n"))
}

/// Pulls the reserved `healthy` and `health` keys out of the incoming
/// status body and returns them alongside the rest of the payload
/// (`extra`). Validates types per the contract:
///
/// - missing or `null` body → `healthy = true`, `health = []`, `extra = {}`
/// - `healthy` absent ⇒ `true` (legacy compat — non-negotiable, this is
///   what stops every legacy server from false-positiving unhealthy on
///   the day we deploy)
/// - `healthy` present must be a bool
/// - `health` if present must be an array of objects, each with
///   `check: non-empty string` and **exactly one** of
///   `result: "passed" | "warning" | "failed" | "broken" | "skipped"`
///   (current bestool) or `healthy: bool` (legacy). An unrecognised
///   `result` string is a 400 — canopy ships before any bestool that
///   adds enum values. Other fields on each entry are passed through
///   verbatim.
fn split_health_from_extra(
	raw: serde_json::Value,
) -> Result<(bool, Option<serde_json::Value>, serde_json::Value)> {
	let mut obj = match raw {
		serde_json::Value::Null => serde_json::Map::new(),
		serde_json::Value::Object(m) => m,
		_ => {
			return Err(AppError::BadRequest(
				"status body must be a JSON object (or null/empty)".into(),
			));
		}
	};

	let healthy = match obj.remove("healthy") {
		None => true,
		Some(serde_json::Value::Bool(b)) => b,
		Some(_) => return Err(AppError::BadRequest("`healthy` must be a boolean".into())),
	};

	// A push without a `health` key is the retired legacy format. We don't
	// reject it here — the caller decides, per the server's
	// `allow_legacy_status` flag, whether to accept it (reachability-only,
	// carrying prior healthchecks forward) or 400 it.
	let Some(health_value) = obj.remove("health") else {
		return Ok((healthy, None, serde_json::Value::Object(obj)));
	};
	let health_arr = match health_value {
		serde_json::Value::Array(a) => a,
		_ => return Err(AppError::BadRequest("`health` must be an array".into())),
	};
	for (idx, entry) in health_arr.iter().enumerate() {
		let Some(entry_obj) = entry.as_object() else {
			return Err(AppError::BadRequest(format!(
				"`health[{idx}]` must be an object",
			)));
		};
		match entry_obj.get("check") {
			Some(serde_json::Value::String(s)) if !s.is_empty() => {}
			Some(_) | None => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}].check` must be a non-empty string",
				)));
			}
		}
		match (entry_obj.get("result"), entry_obj.get("healthy")) {
			(Some(_), Some(_)) => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}]` must not have both `result` and `healthy`",
				)));
			}
			(Some(serde_json::Value::String(s)), None) => {
				if s.parse::<CheckResult>().is_err() {
					return Err(AppError::BadRequest(format!(
						"`health[{idx}].result` must be one of passed, warning, failed, broken, skipped",
					)));
				}
			}
			(Some(_), None) => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}].result` must be a string",
				)));
			}
			(None, Some(serde_json::Value::Bool(_))) => {}
			(None, Some(_)) => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}].healthy` must be a boolean",
				)));
			}
			(None, None) => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}]` must have a `result` (or legacy `healthy`)",
				)));
			}
		}
	}

	Ok((
		healthy,
		Some(serde_json::Value::Array(health_arr)),
		serde_json::Value::Object(obj),
	))
}
