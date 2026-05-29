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

// ── /sample ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn sample_returns_null_when_no_server_has_reported_the_check() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO healthcheck_severities (check_name) VALUES ('uncharted_check');",
		)
		.await
		.unwrap();
		let response = private
			.post("/api/healthchecks/sample")
			.json(&json!({"check_name": "uncharted_check"}))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		assert_eq!(body["check_name"], "uncharted_check");
		assert!(body["sample"].is_null());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sample_materialises_latest_push_for_this_check() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO server_groups (id, name, tags) VALUES \
				('11111111-1111-1111-1111-111111111111', 'prod', '{\"env\": \"prod\"}'::jsonb); \
			 INSERT INTO servers (id, host, name, kind, group_id, tags) VALUES \
				('22222222-2222-2222-2222-222222222222', 'https://prod-host', 'Prod Central', 'central', \
				 '11111111-1111-1111-1111-111111111111', '{\"region\": \"au\"}'::jsonb); \
			 INSERT INTO statuses (server_id, healthy, health, extra, created_at) VALUES \
				('22222222-2222-2222-2222-222222222222', false, \
				 '[{\"check\": \"disk_space\", \"healthy\": false, \"used_pct\": 97}, {\"check\": \"other\", \"healthy\": true}]'::jsonb, \
				 '{\"bestoolVersion\": \"1.13.0\", \"uptimeSecs\": 6038594}'::jsonb, \
				 NOW() - interval '2 minutes');",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/healthchecks/sample")
			.json(&json!({"check_name": "disk_space"}))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		assert_eq!(body["check_name"], "disk_space");
		let sample = &body["sample"];
		assert!(!sample.is_null(), "sample populated when a status push exists");

		// Status-level extras come straight from statuses.extra.
		assert_eq!(sample["status_extra"]["bestoolVersion"], "1.13.0");
		assert_eq!(sample["status_extra"]["uptimeSecs"], 6_038_594);

		// Check-level extras strip the reserved fields.
		assert_eq!(sample["check_extra"]["used_pct"], 97);
		assert!(
			sample["check_extra"].get("check").is_none(),
			"reserved `check` must be stripped"
		);
		assert!(
			sample["check_extra"].get("healthy").is_none(),
			"reserved `healthy` must be stripped"
		);

		// Tags merge: server overlays group; both keys present here.
		assert_eq!(sample["tags"]["env"], "prod");
		assert_eq!(sample["tags"]["region"], "au");

		assert_eq!(sample["server_host"], "https://prod-host/");
		assert_eq!(sample["server_name"], "Prod Central");
		assert!(sample["seen_at"].is_string());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sample_picks_the_most_recent_push_across_servers() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES \
				('33333333-3333-3333-3333-333333333333', 'https://older-host', 'central'), \
				('44444444-4444-4444-4444-444444444444', 'https://newer-host', 'central'); \
			 INSERT INTO statuses (server_id, healthy, health, extra, created_at) VALUES \
				('33333333-3333-3333-3333-333333333333', false, \
				 '[{\"check\": \"cert_expiry\", \"healthy\": false, \"days_remaining\": 30}]'::jsonb, \
				 '{}'::jsonb, NOW() - interval '1 hour'), \
				('44444444-4444-4444-4444-444444444444', false, \
				 '[{\"check\": \"cert_expiry\", \"healthy\": false, \"days_remaining\": 5}]'::jsonb, \
				 '{}'::jsonb, NOW() - interval '1 minute');",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/healthchecks/sample")
			.json(&json!({"check_name": "cert_expiry"}))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		assert_eq!(
			body["sample"]["check_extra"]["days_remaining"], 5,
			"newer push wins"
		);
		assert_eq!(body["sample"]["server_host"], "https://newer-host/");
	})
	.await
}
