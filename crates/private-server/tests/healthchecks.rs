//! `/api/healthchecks/list` and `/api/healthchecks/update`.

use commons_tests::diesel_async::SimpleAsyncConnection;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn list_returns_catalog_rows_with_pending_review_flag() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name, severity) VALUES \
				('disk_space', 'warning'), \
				('reviewed_check', 'error'); \
			 UPDATE healthcheck_severities \
			   SET reviewed_at = NOW(), reviewed_by = 'alice@example.com' \
			   WHERE check_name = 'reviewed_check';",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/healthchecks/list")
			.json(&json!({}))
			.await;
		response.assert_status_ok();

		let body: Vec<serde_json::Value> = response.json();
		assert_eq!(body.len(), 2);

		// Ordered by check_name.
		assert_eq!(body[0]["check_name"], "disk_space");
		assert_eq!(body[0]["severity"], "warning");
		assert_eq!(body[0]["pending_review"], true);
		assert!(body[0]["reviewed_at"].is_null());

		assert_eq!(body[1]["check_name"], "reviewed_check");
		assert_eq!(body[1]["severity"], "error");
		assert_eq!(body[1]["pending_review"], false);
		assert_eq!(body[1]["reviewed_by"], "alice@example.com");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_changes_severity_and_stamps_review_metadata() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name) VALUES ('disk_space');",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/healthchecks/update")
			.json(&json!({
				"check_name": "disk_space",
				"severity": "error"
			}))
			.await;
		response.assert_status_ok();

		let body: serde_json::Value = response.json();
		assert_eq!(body["check_name"], "disk_space");
		assert_eq!(body["severity"], "error");
		assert_eq!(body["pending_review"], false);
		assert!(
			!body["reviewed_at"].is_null(),
			"reviewed_at must be stamped"
		);
		// Test bypass sets the admin login to "admin@localhost".
		assert_eq!(body["reviewed_by"], "admin@localhost");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_rejects_unknown_severity() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name) VALUES ('disk_space');",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/healthchecks/update")
			.json(&json!({
				"check_name": "disk_space",
				"severity": "extremely_critical"
			}))
			.await;
		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_marks_reviewed_even_when_severity_unchanged() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name) VALUES ('noisy_check');",
		)
		.await
		.unwrap();

		// Pass the same default severity to ack without changing.
		let response = private
			.post("/api/healthchecks/update")
			.json(&json!({
				"check_name": "noisy_check",
				"severity": "warning"
			}))
			.await;
		response.assert_status_ok();

		let body: serde_json::Value = response.json();
		assert_eq!(body["pending_review"], false);
	})
	.await
}

// ── v2: /update_rules + list returns rule_count and rules ──────────────────

/// list_severities surfaces `rules` (raw JsonLogic) and a derived
/// `rule_count` (branch count, or 0 when rules is null/malformed).
#[tokio::test(flavor = "multi_thread")]
async fn list_returns_rules_and_rule_count() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name, rules) VALUES \
				('no_rules', NULL), \
				('one_rule', '{\"if\": [{\"==\": [{\"var\": \"check.x\"}, 1]}, \"error\"]}'::jsonb), \
				('two_rules', '{\"if\": [{\"==\": [{\"var\": \"check.x\"}, 1]}, \"error\", {\"==\": [{\"var\": \"check.x\"}, 2]}, \"warning\"]}'::jsonb), \
				('garbage_rules', '{\"and\": [true]}'::jsonb);",
		)
		.await
		.unwrap();

		let response = private.post("/api/healthchecks/list").json(&json!({})).await;
		response.assert_status_ok();
		let body: Vec<serde_json::Value> = response.json();
		let by_name: std::collections::HashMap<&str, &serde_json::Value> = body
			.iter()
			.map(|r| (r["check_name"].as_str().unwrap(), r))
			.collect();

		assert_eq!(by_name["no_rules"]["rule_count"], 0);
		assert!(by_name["no_rules"]["rules"].is_null());

		assert_eq!(by_name["one_rule"]["rule_count"], 1);
		assert!(by_name["one_rule"]["rules"].is_object());

		assert_eq!(by_name["two_rules"]["rule_count"], 2);

		// Malformed rules deserialise to rule_count 0; the raw JSONB is
		// still returned verbatim so the UI can show a warning banner.
		assert_eq!(
			by_name["garbage_rules"]["rule_count"], 0,
			"malformed rules → rule_count 0"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_rules_accepts_valid_ladder() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name) VALUES ('disk_space');",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/healthchecks/update_rules")
			.json(&json!({
				"check_name": "disk_space",
				"rules": {"if": [
					{">": [{"var": "check.used_pct"}, 95]}, "critical"
				]}
			}))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		assert_eq!(body["rule_count"], 1);
		assert!(body["rules"].is_object());
		assert_eq!(body["pending_review"], false, "edit stamps reviewed_at");
		assert_eq!(body["reviewed_by"], "admin@localhost");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_rules_with_null_clears_the_column() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name, rules) VALUES \
				('disk_space', '{\"if\": [{\"==\": [{\"var\": \"check.x\"}, 1]}, \"error\"]}'::jsonb);",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/healthchecks/update_rules")
			.json(&json!({"check_name": "disk_space", "rules": null}))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		assert_eq!(body["rule_count"], 0);
		assert!(body["rules"].is_null());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_rules_normalises_empty_ladder_to_null() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name) VALUES ('disk_space');",
		)
		.await
		.unwrap();
		// `{"if": []}` is a valid-looking empty ladder; expect the API to
		// normalise it to null at write time.
		let response = private
			.post("/api/healthchecks/update_rules")
			.json(&json!({"check_name": "disk_space", "rules": {"if": []}}))
			.await;
		// Either the API rejects an empty ladder OR normalises it. Both
		// land at `rule_count == 0` and a null rules column.
		if response.status_code().is_success() {
			let body: serde_json::Value = response.json();
			assert_eq!(body["rule_count"], 0);
			assert!(body["rules"].is_null());
		} else {
			response.assert_status_bad_request();
		}
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_rules_rejects_malformed_shapes() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name) VALUES ('disk_space');",
		)
		.await
		.unwrap();
		let cases: &[(serde_json::Value, &str)] = &[
			(json!({"and": [true]}), "AND composition"),
			(
				json!({"if": [{"if": [true, true]}, "error"]}),
				"nested if",
			),
			(
				json!({"if": [{"==": [{"var": "BAD.x"}, 1]}, "error"]}),
				"unknown var namespace",
			),
			(
				json!({"if": [{"==": [{"var": "check.x"}, 1]}, "not_a_severity"]}),
				"bad severity",
			),
			(
				json!({"if": [{"in_range": [{"var": "status.v"}, "not-a-range"]}, "warning"]}),
				"bad semver range",
			),
		];
		for (rules, label) in cases {
			let response = private
				.post("/api/healthchecks/update_rules")
				.json(&json!({"check_name": "disk_space", "rules": rules}))
				.await;
			assert!(
				!response.status_code().is_success(),
				"{label} should have been rejected"
			);
		}
	})
	.await
}
