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
