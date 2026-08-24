//! Status-push ingestion: parsing a pushed body, recording it, and filing the
//! checks it carries.
//!
//! One core with two callers. The public-server's `POST /status/{id}` handler
//! is one; the relay connection worker is the other, because a relay's harvest
//! filing **is** a status-push body (spec `K8S`, "Checks harvested for the
//! server"). A Kubernetes server and a server that pushes its own reports
//! therefore share one catalog entry and one policy per check and cannot drift
//! into subtly different checks — not because two implementations are kept in
//! agreement, but because there is one.
//!
//! Everything here is about a *server's own* checks. Substrate checks have no
//! push analogue and go through `database::issues::file_check` directly.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr as _;

use commons_errors::{AppError, Result};
use commons_types::{Uuid, source::IngestMode, status::CheckResult, version::VersionStr};
use database::{
	check_policies::{CheckPolicy, EvaluationContext, GradedResult},
	diesel_async::{AsyncConnection, AsyncPgConnection},
	issues::{CheckStateStamp, Issue, NewEvent},
	servers::Server,
	source_policies::SourcePolicy,
	statuses::{NewStatus, Status},
};

/// The source a push is attributed to when it names none: the reporter
/// deployed before the `source` field existed. Also the migration value for
/// pre-source history. Transitional — the field will become mandatory.
pub const DEFAULT_SOURCE: &str = "alertd";
/// The source legacy-format pushes (no `health` array) are attributed to:
/// they come from Tamanu's own direct reporting, not from alertd.
pub const LEGACY_SOURCE: &str = "tamanu";
/// The synthetic check a legacy push reports: a liveness heartbeat,
/// always passing on receipt. Its value is that it stops — a Tamanu
/// server that goes quiet trips the source-staleness net.
const LEGACY_CHECK: &str = "tasks";
/// Source names a push may not claim: `canopy` is canopy's own
/// determinations (reachability sweep etc.), `manual` is operator-entered,
/// and `kubernetes` is a relay's substrate checks — reserved from the device
/// API because no ordinary device can determine them (spec `K8S`).
const RESERVED_SOURCES: &[&str] = &[
	database::statuses::CANOPY_SOURCE,
	"manual",
	commons_types::source::SUBSTRATE_SOURCE,
];
/// Prefix for per-check refs. Each check is filed at
/// `(<source>, health/<check_name>)` — one thread per check, brokenness
/// included (a broken check retains the previous definite result's
/// contribution while additionally warning that the check is broken).
const HEALTH_REF: &str = "health";

/// A push body, validated and split into the parts ingestion works on.
///
/// Producing this is the only place the payload contract is interpreted, so
/// the HTTP path and the relay path cannot disagree about what a push means.
#[derive(Debug, Clone)]
pub struct ParsedPush {
	/// The reporting source, defaulted and validated.
	pub source: String,
	pub healthy: bool,
	/// The `health` array. A legacy body (no `health` key at all) has already
	/// become the `tamanu`/`tasks` heartbeat by the time it is here.
	pub health: serde_json::Value,
	/// Everything else in the body, stored verbatim.
	pub extra: serde_json::Value,
	/// The Tamanu version this push reports, if any.
	pub version: Option<VersionStr>,
}

/// Whether a push was recorded, which the source's ingest policy decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ingested {
	Recorded,
	/// The source is set to `ignore`: the push was accepted but nothing was
	/// recorded. Callers still answer normally — the response comes from
	/// server state, not from the push, so an ignored reporter keeps working.
	Ignored,
}

/// Validate a pushed body and resolve the version it reports.
///
/// `version_header` is the legacy `X-Version` fallback, for reporters that
/// predate carrying the version in the body; a caller with no such header
/// (the relay) passes `None`.
pub fn parse_push(
	raw: serde_json::Value,
	version_header: Option<VersionStr>,
) -> Result<ParsedPush> {
	let (source, healthy, health, extra) = split_health_from_extra(raw)?;
	let version = resolve_version(&extra, version_header);

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

	Ok(ParsedPush {
		source,
		healthy,
		health,
		extra,
		version,
	})
}

/// Record a parsed push against a server and file the checks it carries.
///
/// Consults the source's ingest policy first: a denied source is rejected
/// outright, an ignored source is accepted without recording. The recording
/// itself is one transaction — the status row, the source's server-wide
/// reported detail, and the check filings land together or not at all.
///
/// `device_id` is provenance: which device reported this. For a relay filing
/// that is the relay, which is not the server's own device and is not meant
/// to be.
pub async fn ingest_push(
	conn: &mut AsyncPgConnection,
	server: &Server,
	device_id: Uuid,
	parsed: &ParsedPush,
	tags: &HashMap<String, serde_json::Value>,
) -> Result<Ingested> {
	// Ingest gating (see CHK "Source policy"): a denied source's push is
	// rejected outright; an ignored source's push is accepted but its data
	// is not recorded.
	match SourcePolicy::ingest_for(conn, &parsed.source).await? {
		IngestMode::Allow => {}
		IngestMode::Ignore => return Ok(Ingested::Ignored),
		IngestMode::Deny => return Err(AppError::IngestDenied(parsed.source.clone())),
	}

	let server_id = server.id;
	let group_id = server.group_id;

	// Insert + file events atomically. NewEvent::save itself opens
	// a transaction; diesel-async nests it as a SAVEPOINT.
	conn.transaction::<_, AppError, _>(async |conn| {
		let status = NewStatus {
			server_id,
			device_id: Some(device_id),
			version: parsed.version.clone(),
			extra: parsed.extra.clone(),
			healthy: parsed.healthy,
			health: parsed.health.clone(),
			source: parsed.source.clone(),
		}
		.save(conn)
		.await?;

		// This source's current server-wide detail, replacing what it
		// last reported. The push is the source's whole truth, the same
		// rule its checks follow just below.
		// spec: FIG#sourcing
		database::reported_detail::ReportedDetail::record(
			conn,
			server_id,
			&status.source,
			&status.extra,
			status.version.as_ref(),
		)
		.await?;

		file_health_events(conn, server_id, group_id, Some(device_id), &status, tags).await?;

		Ok(())
	})
	.await?;

	Ok(Ingested::Recorded)
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
	group_id: Option<Uuid>,
	device_id: Option<Uuid>,
	status: &Status,
	tags: &HashMap<String, serde_json::Value>,
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
	for (check, (result, entry)) in &curr_check_results {
		// Strip the reserved `check` / `healthy` keys, and replace any
		// wire-form `result` with the normalised value so rules see a
		// uniform `check.result` even for legacy (`healthy: bool`)
		// payloads.
		let mut check_extra = (*entry).clone();
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
		let graded = CheckPolicy::apply_scoped(
			conn,
			&status.source,
			check,
			*result,
			&ctx,
			Some(server_id),
			group_id,
		)
		.await?;
		effective.insert(check, graded);
	}

	// The pushing source's previously-open issues: consulted for close
	// messages ("recovered" vs "was never trouble") and for the
	// unmentioned-check closes below.
	let health_prefix = format!("{HEALTH_REF}/");
	let previously_active: BTreeSet<String> =
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
	// definite result: the filing retains an open effective failure's
	// contribution, or counts as a warning when there was nothing to
	// retain (broken contributes as a warning in the rollups).
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
		let (effective, escalates, active, description, message) = match graded.effective {
			CheckResult::Failed => (
				CheckResult::Failed,
				graded.escalates,
				true,
				Some(format!("Health check '{check}' failed")),
				None,
			),
			CheckResult::Warning => (
				CheckResult::Warning,
				graded.escalates,
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
				.filter(|i| i.active && i.effective_result == Some(CheckResult::Failed));
				let (effective, escalates) = match retained {
					Some(prior) => (CheckResult::Failed, prior.escalates),
					None => (CheckResult::Broken, graded.escalates),
				};
				(
					effective,
					escalates,
					true,
					Some(format!("Health check '{check}' is broken")),
					None,
				)
			}
			CheckResult::Passed => (
				CheckResult::Passed,
				graded.escalates,
				false,
				None,
				Some(if was_active {
					format!("Health check '{check}' recovered")
				} else {
					format!("Health check '{check}' passing")
				}),
			),
			CheckResult::Skipped => (
				CheckResult::Skipped,
				graded.escalates,
				false,
				None,
				Some(if was_active {
					format!("Health check '{check}' is now skipped")
				} else {
					format!("Health check '{check}' skipped")
				}),
			),
		};
		let (observed, entry) = curr_check_results[*check];
		let stamp = CheckStateStamp {
			check: (*check).clone(),
			observed,
			effective,
			escalates,
			detail: Some(serde_json::Value::Object(entry.clone())),
		};
		NewEvent {
			source: status.source.clone(),
			r#ref: format!("{HEALTH_REF}/{check}"),
			description,
			message: message
				.or_else(|| per_check_description(entry))
				.unwrap_or_default(),
			active: Some(active),
			occurred_at,
		}
		.save_with_state(conn, server_id, device_id, Some(&stamp), true)
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
			escalates: false,
			detail: None,
		};
		NewEvent {
			source: status.source.clone(),
			r#ref: format!("{HEALTH_REF}/{check}"),
			description: None,
			message: format!("Health check '{check}' recovered"),
			active: Some(false),
			occurred_at,
		}
		.save_with_state(conn, server_id, device_id, Some(&stamp), true)
		.await?;
	}

	// Incident (re-)evaluation is deferred off this request: the issue state
	// above is recorded synchronously, but the incident work — which takes
	// the per-group `server_groups` lock — is handed to the reeval worker so
	// concurrent check-ins never convoy on that lock. Only grouped servers
	// participate in incidents.
	if group_id.is_some() {
		database::issues::enqueue_incident_reeval(conn, server_id).await?;
	}

	Ok(())
}

/// Normalised result of every well-formed check in a `health[]` blob.
/// Anything malformed (non-object entry, missing/invalid `check`, no
/// resolvable result) is ignored — the ingestion path validates on
/// the way in, so by the time we read it back from the DB we're either
/// looking at our own well-formed data or at historical pre-contract
/// rows where missing means absent. Reads both the `result` enum form
/// and the legacy `healthy: bool` form via [`CheckResult::from_entry`].
fn collect_check_results(
	health: &serde_json::Value,
) -> BTreeMap<String, (CheckResult, &serde_json::Map<String, serde_json::Value>)> {
	let Some(arr) = health.as_array() else {
		return BTreeMap::new();
	};
	arr.iter()
		.filter_map(|e| {
			let obj = e.as_object()?;
			let check = obj.get("check")?.as_str()?;
			let result = CheckResult::from_entry(obj)?;
			Some((check.to_string(), (result, obj)))
		})
		.collect()
}

fn per_check_description(entry: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
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

/// Pulls the reserved `source`, `healthy`, and `health` keys out of the
/// incoming status body and returns them alongside the rest of the payload
/// (`extra`). Validates types per the contract:
///
/// - missing or `null` body → `source = alertd`, `healthy = true`,
///   `health = []`, `extra = {}`
/// - `source` absent ⇒ `alertd` (transitional — the field will become
///   mandatory); present must be a non-empty string and not one of the
///   reserved names (`canopy`, `manual`, `kubernetes`)
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

#[cfg(test)]
mod tests {
	use super::*;

	/// The `kubernetes` source is a relay's own, determined about the
	/// substrate, so no device may claim it on a push (spec `K8S`).
	#[test]
	fn a_push_cannot_claim_a_reserved_source() {
		for source in ["canopy", "manual", "kubernetes", "KUBERNETES"] {
			let raw = serde_json::json!({"source": source, "health": []});
			let err = parse_push(raw, None).expect_err("{source} must be refused");
			assert!(matches!(err, AppError::BadRequest(_)), "got {err:?}");
		}
	}

	#[test]
	fn a_push_naming_no_source_is_attributed_to_alertd() {
		let parsed = parse_push(serde_json::json!({"health": []}), None).unwrap();
		assert_eq!(parsed.source, DEFAULT_SOURCE);
	}

	/// A body with no `health` key at all is Tamanu's legacy direct report,
	/// which becomes the always-passing heartbeat rather than an empty push.
	#[test]
	fn a_legacy_body_becomes_the_tamanu_heartbeat() {
		let parsed = parse_push(serde_json::json!({}), None).unwrap();
		assert_eq!(parsed.source, LEGACY_SOURCE);
		assert_eq!(
			parsed.health,
			serde_json::json!([{"check": "tasks", "result": "passed"}]),
		);
	}

	#[test]
	fn the_body_version_wins_over_the_legacy_header() {
		let raw = serde_json::json!({"health": [], "tamanuVersion": "2.30.1"});
		let parsed = parse_push(raw, Some(VersionStr::from_str("1.0.0").unwrap())).unwrap();
		assert_eq!(parsed.version.unwrap().to_string(), "2.30.1");
	}
}
