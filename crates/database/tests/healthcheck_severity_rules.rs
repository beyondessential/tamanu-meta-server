//! v2 conditional-rule model: JsonLogic if-ladder serde + evaluator +
//! `severity_for` integration. The model is the single source of truth
//! for "given a status push, what severity does this check fail at?"
//! See `docs/plans/healthcheck-severity-rules-v2.md`.

use commons_types::issue::Severity;
use database::healthcheck_severities::{EvaluationContext, HealthcheckSeverity, IfLadder};
use serde_json::json;
use std::collections::HashMap;

fn empty_ctx<'a>(
	check_extra: &'a serde_json::Map<String, serde_json::Value>,
	status_extra: &'a serde_json::Map<String, serde_json::Value>,
	tags: &'a HashMap<String, serde_json::Value>,
) -> EvaluationContext<'a> {
	EvaluationContext {
		status_extra,
		check_extra,
		tags,
	}
}

// ── Serde ────────────────────────────────────────────────────────────────

#[test]
fn deserialise_accepts_all_supported_ops() {
	for op in ["==", "!=", "<", "<=", ">", ">="] {
		let json = json!({"if": [{op: [{"var": "check.x"}, 10]}, "error"]});
		serde_json::from_value::<IfLadder>(json).unwrap_or_else(|e| panic!("op {op} failed: {e}"));
	}
	let in_range = json!({"if": [
		{"in_range": [{"var": "status.bestoolVersion"}, ">=2.4.0 <2.5.4"]},
		"warning"
	]});
	serde_json::from_value::<IfLadder>(in_range).expect("in_range");
}

#[test]
fn deserialise_rejects_composition_and_unknown_ops() {
	let cases: &[(serde_json::Value, &str)] = &[
		(json!({"if": [{"and": [true, true]}, "error"]}), "and"),
		(json!({"if": [{"or": [true, true]}, "error"]}), "or"),
		(json!({"if": [{"!": [true]}, "error"]}), "not"),
		(json!({"if": [{"if": [true, true]}, "error"]}), "nested if"),
		(
			json!({"if": [{"foo": [{"var": "check.x"}, 1]}, "error"]}),
			"unknown",
		),
	];
	for (val, label) in cases {
		serde_json::from_value::<IfLadder>(val.clone())
			.err()
			.unwrap_or_else(|| panic!("{label} should have been rejected"));
	}
}

#[test]
fn deserialise_rejects_malformed_shapes() {
	let cases: &[(serde_json::Value, &str)] = &[
		(json!({"if": []}), "empty args"),
		(
			json!({"if": [{"==": [{"var": "check.x"}, 1]}]}),
			"odd-length",
		),
		(
			json!({"if": [{"==": [{"var": "check.x"}, 1]}, "warning", "trailing"]}),
			"trailing else",
		),
		(json!({"not_if": []}), "wrong top-level op"),
		(
			json!({"if": [{"==": [{"var": "check.x"}, 1]}, "not_a_severity"]}),
			"bad severity",
		),
		(
			json!({"if": [{"==": [{"var": "BAD.field"}, 1]}, "error"]}),
			"unknown var namespace",
		),
		(
			json!({"if": [{"==": [{"var": "check..bad"}, 1]}, "error"]}),
			"empty field segment",
		),
	];
	for (val, label) in cases {
		serde_json::from_value::<IfLadder>(val.clone())
			.err()
			.unwrap_or_else(|| panic!("{label} should have been rejected"));
	}
}

#[test]
fn round_trip_preserves_shape() {
	let original = json!({"if": [
		{"in_range": [{"var": "status.bestoolVersion"}, ">=2.4.0 <2.5.4"]}, "warning",
		{">": [{"var": "check.used_pct"}, 95]}, "critical",
		{"==": [{"var": "tag.environment"}, "prod"]}, "error"
	]});
	let ladder: IfLadder = serde_json::from_value(original.clone()).expect("parse");
	let back = serde_json::to_value(&ladder).expect("serialise");
	assert_eq!(back, original);
}

// ── Evaluator ────────────────────────────────────────────────────────────

#[test]
fn in_range_matches_inside_misses_outside_and_falses_on_non_semver() {
	let ladder: IfLadder = serde_json::from_value(json!({"if": [
		{"in_range": [{"var": "status.bestoolVersion"}, ">=2.4.0 <2.5.4"]}, "warning"
	]}))
	.unwrap();

	let check_extra = serde_json::Map::new();
	let tags = HashMap::new();

	let mut status = serde_json::Map::new();
	status.insert("bestoolVersion".into(), json!("2.4.7"));
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check_extra, &status, &tags)),
		Some(Severity::Warning),
	);

	status.insert("bestoolVersion".into(), json!("2.5.4"));
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check_extra, &status, &tags)),
		None,
	);

	// Non-semver LHS (e.g. windows osVersion) → false.
	status.insert("bestoolVersion".into(), json!("10 (17763)"));
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check_extra, &status, &tags)),
		None,
	);

	// Missing field → false.
	let status_missing = serde_json::Map::new();
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check_extra, &status_missing, &tags)),
		None,
	);
}

#[test]
fn numeric_ops_coerce_string_of_digits() {
	let ladder: IfLadder = serde_json::from_value(json!({"if": [
		{">": [{"var": "check.value"}, 95]}, "critical"
	]}))
	.unwrap();
	let status = serde_json::Map::new();
	let tags = HashMap::new();

	// Real number > 95 → matches.
	let mut check = serde_json::Map::new();
	check.insert("value".into(), json!(97));
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check, &status, &tags)),
		Some(Severity::Critical),
	);

	// String of digits > 95 (bestool sometimes does this) → matches.
	check.insert("value".into(), json!("100"));
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check, &status, &tags)),
		Some(Severity::Critical),
	);

	// Non-numeric string → false.
	check.insert("value".into(), json!("abc"));
	assert_eq!(ladder.evaluate(&empty_ctx(&check, &status, &tags)), None);

	// Missing field → false.
	let missing = serde_json::Map::new();
	assert_eq!(ladder.evaluate(&empty_ctx(&missing, &status, &tags)), None,);
}

#[test]
fn eq_neq_handle_bool_string_numeric() {
	let cases: &[(serde_json::Value, serde_json::Value, bool, &str)] = &[
		(json!(true), json!(true), true, "bool eq"),
		(json!(true), json!(false), false, "bool neq"),
		(json!("prod"), json!("prod"), true, "string eq"),
		(json!("prod"), json!("staging"), false, "string neq"),
		(json!(5), json!(5.0), true, "numeric coercion 5 == 5.0"),
		(
			json!("21608625"),
			json!(21_608_625),
			true,
			"string-of-digits eq number",
		),
	];
	for (lhs, rhs, want_eq, label) in cases {
		let ladder: IfLadder = serde_json::from_value(json!({"if": [
			{"==": [{"var": "check.value"}, rhs.clone()]}, "error"
		]}))
		.unwrap();
		let mut check = serde_json::Map::new();
		check.insert("value".into(), lhs.clone());
		let status = serde_json::Map::new();
		let tags = HashMap::new();
		let matched = ladder
			.evaluate(&empty_ctx(&check, &status, &tags))
			.is_some();
		assert_eq!(matched, *want_eq, "{label}");
	}
}

#[test]
fn tag_namespace_resolves_against_tag_map() {
	let ladder: IfLadder = serde_json::from_value(json!({"if": [
		{"==": [{"var": "tag.environment"}, "prod"]}, "error"
	]}))
	.unwrap();
	let check = serde_json::Map::new();
	let status = serde_json::Map::new();

	let mut tags = HashMap::new();
	tags.insert(
		"environment".to_string(),
		serde_json::Value::String("prod".into()),
	);
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check, &status, &tags)),
		Some(Severity::Error),
	);

	tags.insert(
		"environment".to_string(),
		serde_json::Value::String("staging".into()),
	);
	assert_eq!(ladder.evaluate(&empty_ctx(&check, &status, &tags)), None);

	tags.clear();
	assert_eq!(ladder.evaluate(&empty_ctx(&check, &status, &tags)), None);
}

#[test]
fn first_match_wins() {
	let ladder: IfLadder = serde_json::from_value(json!({"if": [
		{"<": [{"var": "check.days_remaining"}, 7]},  "error",
		{"<": [{"var": "check.days_remaining"}, 30]}, "warning"
	]}))
	.unwrap();
	let status = serde_json::Map::new();
	let tags = HashMap::new();
	let mut check = serde_json::Map::new();

	check.insert("days_remaining".into(), json!(3));
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check, &status, &tags)),
		Some(Severity::Error),
	);
	check.insert("days_remaining".into(), json!(15));
	assert_eq!(
		ladder.evaluate(&empty_ctx(&check, &status, &tags)),
		Some(Severity::Warning),
	);
	check.insert("days_remaining".into(), json!(60));
	assert_eq!(ladder.evaluate(&empty_ctx(&check, &status, &tags)), None);
}

// ── severity_for integration ─────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn severity_for_uses_rules_or_falls_back_to_base() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		HealthcheckSeverity::upsert_default(&mut conn, "tamanu_service")
			.await
			.expect("seed");

		let ladder: IfLadder = serde_json::from_value(json!({"if": [
			{"in_range": [{"var": "status.bestoolVersion"}, ">=2.4.0 <2.5.4"]}, "warning"
		]}))
		.unwrap();
		HealthcheckSeverity::update_rules(&mut conn, "tamanu_service", Some(&ladder), "ops")
			.await
			.expect("save rules");

		// Bestool inside the range → ladder fires → Warning.
		let mut status = serde_json::Map::new();
		status.insert("bestoolVersion".into(), json!("2.4.7"));
		let check = serde_json::Map::new();
		let tags = HashMap::new();
		let sev = HealthcheckSeverity::severity_for(
			&mut conn,
			"tamanu_service",
			&empty_ctx(&check, &status, &tags),
		)
		.await
		.expect("severity_for");
		assert_eq!(sev, Severity::Warning);

		// Bestool outside the range → falls back to the base (Warning by default).
		status.insert("bestoolVersion".into(), json!("2.6.0"));
		let sev = HealthcheckSeverity::severity_for(
			&mut conn,
			"tamanu_service",
			&empty_ctx(&check, &status, &tags),
		)
		.await
		.expect("severity_for");
		assert_eq!(sev, Severity::Warning);

		// Bump the base to Error and re-test fallback path.
		HealthcheckSeverity::update(&mut conn, "tamanu_service", Severity::Error, None, "ops")
			.await
			.expect("bump base");
		status.insert("bestoolVersion".into(), json!("2.6.0"));
		let sev = HealthcheckSeverity::severity_for(
			&mut conn,
			"tamanu_service",
			&empty_ctx(&check, &status, &tags),
		)
		.await
		.expect("severity_for");
		assert_eq!(
			sev,
			Severity::Error,
			"falls back to base when no branch matches"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn severity_for_falls_back_when_rules_are_malformed() {
	use diesel::sql_query;
	use diesel::sql_types;
	use diesel_async::RunQueryDsl;

	commons_tests::db::TestDb::run(async |mut conn, _| {
		HealthcheckSeverity::upsert_default(&mut conn, "tamanu_service")
			.await
			.expect("seed");
		// Manually inject garbage into the rules column.
		sql_query(
			"UPDATE healthcheck_severities SET rules = $1::jsonb WHERE check_name = 'tamanu_service'",
		)
		.bind::<sql_types::Text, _>(r#"{"and": [true, true]}"#)
		.execute(&mut conn)
		.await
		.expect("inject garbage");

		let check = serde_json::Map::new();
		let status = serde_json::Map::new();
		let tags = HashMap::new();
		let sev = HealthcheckSeverity::severity_for(
			&mut conn,
			"tamanu_service",
			&empty_ctx(&check, &status, &tags),
		)
		.await
		.expect("severity_for");
		// Catalog default is Warning; malformed rules don't crash, just defer.
		assert_eq!(sev, Severity::Warning);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_rules_clears_column_when_passed_none() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		HealthcheckSeverity::upsert_default(&mut conn, "disk_space")
			.await
			.expect("seed");
		let ladder: IfLadder = serde_json::from_value(json!({"if": [
			{">": [{"var": "check.used_pct"}, 95]}, "critical"
		]}))
		.unwrap();
		let saved =
			HealthcheckSeverity::update_rules(&mut conn, "disk_space", Some(&ladder), "ops")
				.await
				.expect("save");
		assert!(saved.rules.is_some(), "rules should be populated");

		let cleared = HealthcheckSeverity::update_rules(&mut conn, "disk_space", None, "ops")
			.await
			.expect("clear");
		assert!(cleared.rules.is_none(), "rules should be cleared by None");
	})
	.await
}
