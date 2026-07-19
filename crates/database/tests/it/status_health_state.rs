//! `Status::health_state()` rollup over per-check entries, in both the
//! legacy `healthy: bool` and the `result` enum forms.

use commons_types::status::HealthState;
use database::statuses::Status;
use jiff::Timestamp;
use uuid::Uuid;

fn status(healthy: bool, health: serde_json::Value) -> Status {
	Status {
		id: Uuid::nil(),
		created_at: Timestamp::UNIX_EPOCH,
		server_id: Uuid::nil(),
		device_id: None,
		version: None,
		extra: serde_json::json!({}),
		healthy,
		health,
		source: "alertd".into(),
	}
}

#[test]
fn top_level_false_is_unhealthy() {
	assert_eq!(
		status(false, serde_json::json!([])).health_state(),
		HealthState::Unhealthy,
	);
}

#[test]
fn no_entries_is_healthy() {
	assert_eq!(
		status(true, serde_json::json!([])).health_state(),
		HealthState::Healthy,
	);
}

/// Legacy bestool encoded a per-check *warning* as `healthy: false`
/// under top-level `true` (real failures flipped top-level to false),
/// so the legacy bool form rolls up to Warning, not Unhealthy.
#[test]
fn legacy_failing_entry_is_warning() {
	assert_eq!(
		status(
			true,
			serde_json::json!([
				{"check": "a", "healthy": true},
				{"check": "b", "healthy": false},
			])
		)
		.health_state(),
		HealthState::Warning,
	);
}

#[test]
fn result_warning_and_broken_count_toward_warning() {
	for result in ["warning", "broken"] {
		assert_eq!(
			status(true, serde_json::json!([{"check": "a", "result": result}])).health_state(),
			HealthState::Warning,
			"{result}",
		);
	}
}

/// An explicit `result: failed` is what legacy bestool folded into
/// top-level `healthy: false` — incident-class, regardless of the
/// (retiring, absent ⇒ true) top-level flag.
#[test]
fn result_failed_is_unhealthy() {
	assert_eq!(
		status(
			true,
			serde_json::json!([
				{"check": "a", "result": "warning"},
				{"check": "b", "result": "failed"},
			])
		)
		.health_state(),
		HealthState::Unhealthy,
	);
}

#[test]
fn result_passed_and_skipped_do_not_count() {
	assert_eq!(
		status(
			true,
			serde_json::json!([
				{"check": "a", "result": "passed"},
				{"check": "b", "result": "skipped"},
			])
		)
		.health_state(),
		HealthState::Healthy,
	);
}

/// A silenced failing check is treated as skipped: it doesn't drag the
/// server to Unhealthy (or Warning), whatever form it was reported in.
#[test]
fn silenced_failing_check_does_not_count() {
	let silenced = std::collections::BTreeSet::from(["b".to_string()]);
	assert_eq!(
		status(
			true,
			serde_json::json!([
				{"check": "a", "result": "passed"},
				{"check": "b", "result": "failed"},
			])
		)
		.health_state_ignoring(&silenced),
		HealthState::Healthy,
	);
	// Legacy per-check bool form.
	assert_eq!(
		status(
			true,
			serde_json::json!([
				{"check": "a", "healthy": true},
				{"check": "b", "healthy": false},
			])
		)
		.health_state_ignoring(&silenced),
		HealthState::Healthy,
	);
}

/// Silencing one check doesn't excuse the others: an unsilenced failure
/// still rolls up to Unhealthy.
#[test]
fn unsilenced_failing_check_still_counts() {
	let silenced = std::collections::BTreeSet::from(["b".to_string()]);
	assert_eq!(
		status(
			true,
			serde_json::json!([
				{"check": "b", "result": "failed"},
				{"check": "c", "result": "failed"},
			])
		)
		.health_state_ignoring(&silenced),
		HealthState::Unhealthy,
	);
	assert_eq!(
		status(
			true,
			serde_json::json!([
				{"check": "b", "result": "failed"},
				{"check": "c", "result": "warning"},
			])
		)
		.health_state_ignoring(&silenced),
		HealthState::Warning,
	);
}

/// With no silences the two forms agree.
#[test]
fn empty_silence_set_matches_health_state() {
	let st = status(
		true,
		serde_json::json!([
			{"check": "a", "result": "warning"},
			{"check": "b", "result": "failed"},
		]),
	);
	assert_eq!(
		st.health_state_ignoring(&Default::default()),
		st.health_state(),
	);
}

/// The legacy top-level `healthy: false` short-circuit predates
/// per-check results, so it can't be attributed to a silenced check
/// and still wins.
#[test]
fn top_level_false_ignores_silences() {
	let silenced = std::collections::BTreeSet::from(["b".to_string()]);
	assert_eq!(
		status(false, serde_json::json!([{"check": "b", "healthy": false}]))
			.health_state_ignoring(&silenced),
		HealthState::Unhealthy,
	);
}
