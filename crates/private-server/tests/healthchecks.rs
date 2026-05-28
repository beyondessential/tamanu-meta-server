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
