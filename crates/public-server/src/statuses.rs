use std::collections::BTreeSet;

use axum::{
	Json,
	extract::{Path, State},
};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{device_auth::ServerDevice, headers::VersionHeader};
use commons_types::{device::DeviceRole, issue::Severity};
use database::{
	Db,
	devices::Device,
	diesel_async::{AsyncConnection, AsyncPgConnection},
	issues::NewEvent,
	servers::Server,
	statuses::{NewStatus, Status},
};
use serde::Deserialize;
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
	/// historical analysis and the status snapshot UI, but the value
	/// is no longer authoritative: canopy derives the system-healthy
	/// judgement from per-check results, not from this flag. See
	/// `docs/plans/healthcheck-severity-catalog.md` for the rationale.
	pub healthy: Option<bool>,

	/// Per-check breakdown. Each entry must include `check` (non-empty)
	/// and `healthy`; arbitrary additional fields per check (latency,
	/// free disk %, certificate expiry, etc.) are passed through
	/// verbatim and surfaced in the status snapshot UI.
	///
	/// A failing check files (or keeps active) an issue at
	/// `(status, health/<check>)`. Severity is chosen per the
	/// transitional rule documented in `file_health_events`.
	pub health: Option<Vec<HealthCheck>>,

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
	/// Pass/fail for the check.
	pub healthy: bool,
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
/// Prefix for per-check refs. Each failing check is filed at
/// `(status, health/<check_name>)`. There used to be a roll-up
/// issue at `(status, health)` driven by bestool's top-level
/// `healthy` flag — that was retired (see
/// `docs/plans/healthcheck-severity-catalog.md`); the prefix lives
/// on for the per-check refs.
const HEALTH_REF: &str = "health";

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
		description = "Status push. Empty body or JSON `null` are both treated as `{}` (all fields default).",
	),
	responses(
		(status = 200, body = Status),
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
) -> Result<Json<Status>> {
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
	let (healthy, health, extra) = split_health_from_extra(raw)?;

	// Read previous-latest BEFORE the write transaction so we can
	// detect per-check transitions (the close events on recovered
	// checks need to know which checks were failing last time). The
	// snapshot is a small read; no need to hold a lock.
	let prev = Status::latest_for_server(&mut db, server_id).await?;
	let prev_failing_checks = prev
		.as_ref()
		.map(|s| collect_failing_checks(&s.health))
		.unwrap_or_default();

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

			file_health_events(conn, server_id, Some(id), &status, &prev_failing_checks).await?;

			Ok(status)
		})
		.await?;

	Ok(Json(status))
}

/// Per-push event filing. Per-check failures land at
/// `(status, health/<check>)`; recoveries close those issues. The
/// roll-up issue that used to live at `(status, health)` (driven by
/// bestool's top-level `healthy` flag) is retired — see
/// `docs/plans/healthcheck-severity-catalog.md`.
///
/// Per-check severity stays coupled to `status.healthy` for now
/// (Warning while bestool calls itself healthy overall; Error
/// otherwise). v1 of the catalog plan replaces this with a
/// catalog-driven lookup so the choice becomes operator-owned.
async fn file_health_events(
	conn: &mut AsyncPgConnection,
	server_id: Uuid,
	device_id: Option<Uuid>,
	status: &Status,
	prev_failing_checks: &BTreeSet<String>,
) -> Result<()> {
	let curr_failing_checks = collect_failing_checks(&status.health);
	let per_check_severity = if status.healthy {
		Severity::Warning
	} else {
		Severity::Error
	};
	let occurred_at = Some(status.created_at);

	// Per-check opens.
	for check in &curr_failing_checks {
		let entry = find_health_entry(&status.health, check);
		NewEvent {
			source: STATUS_SOURCE.into(),
			r#ref: format!("{HEALTH_REF}/{check}"),
			severity: Some(per_check_severity),
			description: Some(format!("Health check '{check}' failed")),
			message: per_check_description(entry).unwrap_or_default(),
			active: Some(true),
			occurred_at,
		}
		.save(conn, server_id, device_id)
		.await?;
	}

	// Per-check closes: anything that was failing last time but
	// isn't failing now. Two paths roll into this: the server
	// explicitly says `healthy: true` for the check, OR the server
	// stops mentioning the check altogether ("trust the reporter").
	for check in prev_failing_checks.difference(&curr_failing_checks) {
		NewEvent {
			source: STATUS_SOURCE.into(),
			r#ref: format!("{HEALTH_REF}/{check}"),
			severity: Some(Severity::Info),
			description: None,
			message: format!("Health check '{check}' recovered"),
			active: Some(false),
			occurred_at,
		}
		.save(conn, server_id, device_id)
		.await?;
	}

	Ok(())
}

/// Names of checks in a `health[]` blob where `healthy == false`.
/// Anything malformed (non-object entry, missing/invalid `check`,
/// missing/invalid `healthy`) is ignored — the public endpoint
/// validates on the way in, so by the time we read it back from the
/// DB we're either looking at our own well-formed data or at
/// historical pre-contract rows where missing means absent.
fn collect_failing_checks(health: &serde_json::Value) -> BTreeSet<String> {
	let Some(arr) = health.as_array() else {
		return BTreeSet::new();
	};
	arr.iter()
		.filter_map(|e| {
			let obj = e.as_object()?;
			let healthy = obj.get("healthy")?.as_bool()?;
			if healthy {
				return None;
			}
			let check = obj.get("check")?.as_str()?;
			Some(check.to_string())
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
		if k == "check" || k == "healthy" {
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
/// - `health` if present must be an array of objects, each with at
///   least `check: non-empty string` and `healthy: bool`. Other fields
///   on each entry are passed through verbatim.
fn split_health_from_extra(
	raw: serde_json::Value,
) -> Result<(bool, serde_json::Value, serde_json::Value)> {
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

	let health_value = obj
		.remove("health")
		.unwrap_or(serde_json::Value::Array(Default::default()));
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
		match entry_obj.get("healthy") {
			Some(serde_json::Value::Bool(_)) => {}
			Some(_) | None => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}].healthy` must be a boolean",
				)));
			}
		}
	}

	Ok((
		healthy,
		serde_json::Value::Array(health_arr),
		serde_json::Value::Object(obj),
	))
}
