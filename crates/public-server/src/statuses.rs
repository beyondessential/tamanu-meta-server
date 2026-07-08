use std::collections::BTreeMap;
use std::str::FromStr as _;

use axum::{
	Json,
	extract::{Path, State},
};
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{
	backup_jobs::backups_due_now_for_server, device_auth::ServerDevice, headers::VersionHeader,
};
use commons_types::{
	backup::BackupType,
	device::DeviceRole,
	issue::Severity,
	server::TagMap,
	status::{CheckResult, CheckSeverity},
	version::VersionStr,
};
use database::{
	Db,
	check_policies::{CheckPolicy, EvaluationContext, GradedResult},
	devices::Device,
	diesel_async::{AsyncConnection, AsyncPgConnection},
	issues::{CheckStateStamp, Issue, NewEvent},
	servers::Server,
	silenced_refs::silenced_refs_with_prefix,
	statuses::{NewStatus, Status},
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

/// The source a push is attributed to when it names none: the reporter
/// deployed before the `source` field existed. Also the migration value for
/// pre-source history. Transitional — the field will become mandatory.
const DEFAULT_SOURCE: &str = "alertd";
/// The source legacy-format pushes (no `health` array) are attributed to:
/// they come from Tamanu's own direct reporting, not from alertd.
const LEGACY_SOURCE: &str = "tamanu";
/// The synthetic check a legacy push reports: a liveness heartbeat,
/// always passing on receipt. Its value is that it stops — a Tamanu
/// server that goes quiet trips the source-staleness net.
const LEGACY_CHECK: &str = "tasks";
/// Source names a push may not claim: `canopy` is canopy's own
/// determinations (reachability sweep etc.), `manual` is operator-entered.
const RESERVED_SOURCES: &[&str] = &[database::statuses::CANOPY_SOURCE, "manual"];
/// Prefix for per-check refs. Each check is filed at
/// `(<source>, health/<check_name>)` — one thread per check, brokenness
/// included (a broken check retains the previous definite result's
/// contribution while additionally warning that the check is broken).
const HEALTH_REF: &str = "health";

/// The status-push response: only the return-path instructions the device
/// can act on. The stored status record is deliberately not echoed back —
/// the device already has everything it sent.
#[derive(Debug, Serialize, ToSchema)]
pub struct StatusResponse {
	/// Backup types the server should back up now: operator-requested
	/// one-offs plus scheduled backups that are due. Each serializes as a
	/// plain string (e.g. `"tamanu-postgres"`). The device should run each
	/// listed type, then report via `POST /backup-report`; an empty list
	/// means nothing to do.
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
	let (source, healthy, health, extra) = split_health_from_extra(raw)?;

	// The server version canopy tracks (and compares against the published
	// version catalog) is the Tamanu version. Prefer the payload's
	// `tamanuVersion` extra; fall back to the legacy `X-Version` header for
	// reporters that predate carrying it in the body. Either may be absent.
	let version = resolve_version(&extra, current_version.map(|v| v.0));

	// Legacy format (no `health` array): Tamanu's direct reporting. It
	// becomes a heartbeat from the `tamanu` source — a single `tasks`
	// check that always passes on receipt — and flows through the normal
	// path from here, so it records state, registers its catalog entry,
	// and participates in source staleness like any source.
	let (source, health) = match health {
		Some(health) => (source, health),
		None => (
			LEGACY_SOURCE.to_string(),
			serde_json::json!([{ "check": LEGACY_CHECK, "result": "passed" }]),
		),
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
	db.transaction::<_, AppError, _>(async |conn| {
		let status = NewStatus {
			server_id,
			device_id: Some(id),
			version,
			extra,
			healthy,
			health,
			source: source.clone(),
		}
		.save(conn)
		.await?;

		file_health_events(conn, server_id, Some(id), &status, &tags).await?;

		Ok(())
	})
	.await?;

	// Computed after the transaction so checks first seen on this very push
	// (upserted into the catalog above) are already in the map.
	let check_severities =
		effective_check_severities(&mut db, server_id, server.group_id, &source).await?;
	let effective_tags = crate::tags::effective_tags_for_server(&mut db, &server).await?;

	Ok(Json(StatusResponse {
		backup_now,
		check_severities,
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

	let health_prefix = format!("{HEALTH_REF}/");
	for r#ref in silenced_refs_with_prefix(db, server_id, group_id, &health_prefix).await? {
		if let Some(check) = r#ref.strip_prefix(&health_prefix) {
			map.insert(check.to_string(), CheckSeverity::Skip);
		}
	}

	Ok(map)
}

/// Resolve the server version to record on this status. Prefers the payload's
/// `tamanuVersion` extra (the version bestool now carries in the body), parsed
/// as a semver; falls back to the `X-Version` header for reporters that still
/// send it there. Returns `None` when neither is present or parseable — the
/// `statuses.version` column is nullable and every consumer already handles a
/// versionless row.
fn resolve_version(extra: &serde_json::Value, header: Option<VersionStr>) -> Option<VersionStr> {
	extra
		.get("tamanuVersion")
		.and_then(|v| v.as_str())
		.and_then(|s| VersionStr::from_str(s).ok())
		.or(header)
}

/// Per-push event filing. Warning/failed checks land at
/// `(status, health/<check>)`; recoveries close those issues. Broken
/// checks (`result: broken` — the check itself errored, not the
/// system under test) neither confirm nor clear a known failure: the
/// check's issue stays open, retaining its contribution. Skipped checks
/// (`result: skipped` — precondition not met) file nothing and close
/// the check's issue.
///
/// Each check's effective result comes from applying the operator-owned
/// `check_policies` catalog entry for `(source, check)` (see
/// [`CheckPolicy::apply`] for the rules/ceiling contract) to the
/// observed result. Every check seen on a push — whatever its result —
/// upserts a default catalog row so new checks are visible to operators
/// immediately at the default warning ceiling. `status.healthy` is
/// intentionally not consulted: the catalog is canopy's single source
/// of truth for per-check grading.
///
/// Until issues themselves carry results, the effective result maps to
/// the issue severity: failed → error (critical when the policy
/// escalates), warning and broken → warning; passed and skipped file
/// nothing and close prior issues.
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
	// whatever its result. New checks land at the default warning
	// ceiling; operators can review and adjust from the /healthchecks
	// page.
	for check_name in curr_check_results.keys() {
		CheckPolicy::upsert_default(conn, &status.source, check_name).await?;
	}

	// Status-level extras are shared across every per-check evaluation.
	let empty_map = serde_json::Map::new();
	let status_extra = status.extra.as_object().unwrap_or(&empty_map);

	// Grade every check in the push through its policy.
	let mut effective: BTreeMap<&String, GradedResult> = BTreeMap::new();
	for (check, result) in &curr_check_results {
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
		let graded = CheckPolicy::apply(conn, &status.source, check, *result, &ctx).await?;
		effective.insert(check, graded);
	}

	// The pushing source's previously-open issues: consulted for close
	// messages ("recovered" vs "was never trouble") and for the
	// unmentioned-check closes below.
	let health_prefix = format!("{HEALTH_REF}/");
	let previously_active: std::collections::BTreeSet<String> =
		Issue::active_refs_with_prefix(conn, server_id, &status.source, &health_prefix)
			.await?
			.into_iter()
			.filter_map(|r| {
				r.strip_prefix(&health_prefix)
					.map(|check| check.to_string())
			})
			.collect();

	// File every check in the push — passing ones included, so the state
	// row records the current result and when it was last reported. An
	// effective broken result neither confirms nor clears the previous
	// definite result: the filing retains the open issue's contribution
	// (its current severity), or warns that the check is broken when
	// there was nothing to retain.
	//
	// Degraded checks file before recoveries: when one failure swaps for
	// another in a single push, the incoming failure must join the open
	// incident before the outgoing one leaves, or the incident closes
	// and reopens as two.
	let filing_order = effective.iter().filter(|(_, g)| {
		matches!(
			g.effective,
			CheckResult::Warning | CheckResult::Failed | CheckResult::Broken
		)
	});
	let filing_order = filing_order.chain(
		effective
			.iter()
			.filter(|(_, g)| matches!(g.effective, CheckResult::Passed | CheckResult::Skipped)),
	);
	for (check, graded) in filing_order {
		let was_active = previously_active.contains(*check);
		let (severity, active, description, message) = match graded.effective {
			CheckResult::Failed => (
				if graded.escalates {
					Severity::Critical
				} else {
					Severity::Error
				},
				true,
				Some(format!("Health check '{check}' failed")),
				None,
			),
			CheckResult::Warning => (
				Severity::Warning,
				true,
				Some(format!("Health check '{check}' warned")),
				None,
			),
			CheckResult::Broken => {
				let retained = Issue::list_by_source_ref(
					conn,
					&status.source,
					&format!("{HEALTH_REF}/{check}"),
					&[server_id],
				)
				.await?
				.into_iter()
				.next()
				.filter(|i| i.active)
				.map(|i| i.severity)
				.unwrap_or(Severity::Warning);
				(
					retained,
					true,
					Some(format!("Health check '{check}' is broken")),
					None,
				)
			}
			CheckResult::Passed => (
				Severity::Info,
				false,
				None,
				Some(if was_active {
					format!("Health check '{check}' recovered")
				} else {
					format!("Health check '{check}' passing")
				}),
			),
			CheckResult::Skipped => (
				Severity::Info,
				false,
				None,
				Some(if was_active {
					format!("Health check '{check}' is now skipped")
				} else {
					format!("Health check '{check}' skipped")
				}),
			),
		};
		let entry = find_health_entry(&status.health, check);
		let stamp = CheckStateStamp {
			check: (*check).clone(),
			observed: curr_check_results[*check],
			effective: graded.effective,
			detail: entry.cloned().map(serde_json::Value::Object),
		};
		NewEvent {
			source: status.source.clone(),
			r#ref: format!("{HEALTH_REF}/{check}"),
			severity: Some(severity),
			description,
			message: message
				.or_else(|| per_check_description(entry))
				.unwrap_or_default(),
			active: Some(active),
			occurred_at,
		}
		.save_with_state(conn, server_id, device_id, Some(&stamp))
		.await?;
	}

	// Unmentioned closes: a check the source previously reported but
	// omits from this push has recovered ("trust the reporter"). Scoped
	// to the pushing source: one source's push says nothing about
	// another's checks.
	for check in &previously_active {
		if curr_check_results.contains_key(check) {
			continue;
		}
		let stamp = CheckStateStamp {
			check: check.clone(),
			observed: CheckResult::Passed,
			effective: CheckResult::Passed,
			detail: None,
		};
		NewEvent {
			source: status.source.clone(),
			r#ref: format!("{HEALTH_REF}/{check}"),
			severity: Some(Severity::Info),
			description: None,
			message: format!("Health check '{check}' recovered"),
			active: Some(false),
			occurred_at,
		}
		.save_with_state(conn, server_id, device_id, Some(&stamp))
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

/// Pulls the reserved `source`, `healthy`, and `health` keys out of the
/// incoming status body and returns them alongside the rest of the payload
/// (`extra`). Validates types per the contract:
///
/// - missing or `null` body → `source = alertd`, `healthy = true`,
///   `health = []`, `extra = {}`
/// - `source` absent ⇒ `alertd` (transitional — the field will become
///   mandatory); present must be a non-empty string and not one of the
///   reserved names (`canopy`, `manual`)
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
) -> Result<(String, bool, Option<serde_json::Value>, serde_json::Value)> {
	let mut obj = match raw {
		serde_json::Value::Null => serde_json::Map::new(),
		serde_json::Value::Object(m) => m,
		_ => {
			return Err(AppError::BadRequest(
				"status body must be a JSON object (or null/empty)".into(),
			));
		}
	};

	let source = match obj.remove("source") {
		None => DEFAULT_SOURCE.to_string(),
		Some(serde_json::Value::String(s)) if !s.is_empty() => {
			if RESERVED_SOURCES.iter().any(|r| s.eq_ignore_ascii_case(r)) {
				return Err(AppError::BadRequest(format!(
					"`source` must not be a reserved name ({})",
					RESERVED_SOURCES.join(", "),
				)));
			}
			s
		}
		Some(_) => {
			return Err(AppError::BadRequest(
				"`source` must be a non-empty string".into(),
			));
		}
	};

	let healthy = match obj.remove("healthy") {
		None => true,
		Some(serde_json::Value::Bool(b)) => b,
		Some(_) => return Err(AppError::BadRequest("`healthy` must be a boolean".into())),
	};

	// A push without a `health` key is the legacy Tamanu direct-report
	// format; the caller transforms it into the tamanu/tasks heartbeat.
	let Some(health_value) = obj.remove("health") else {
		return Ok((source, healthy, None, serde_json::Value::Object(obj)));
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
		source,
		healthy,
		Some(serde_json::Value::Array(health_arr)),
		serde_json::Value::Object(obj),
	))
}
