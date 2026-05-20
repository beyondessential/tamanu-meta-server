use commons_tests::{diesel_async::SimpleAsyncConnection, server};
use serde_json::Value;

async fn seed_version(conn: &mut commons_tests::diesel_async::AsyncPgConnection) -> &'static str {
	conn.batch_execute(
		"INSERT INTO versions (id, major, minor, patch, status, changelog) VALUES
		('11111111-1111-1111-1111-111111111111', 1, 0, 0, 'published', 'Version 1.0.0')",
	)
	.await
	.unwrap();
	"11111111-1111-1111-1111-111111111111"
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_is_true_when_no_known_issues() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_version(&mut conn).await;

		let detail: Value = private
			.post("/api/versions/get_version_detail")
			.json(&serde_json::json!({ "version": "1.0.0" }))
			.await
			.json();
		assert_eq!(detail.get("ready").and_then(|v| v.as_bool()), Some(true));
		assert_eq!(
			detail.get("known_issues").and_then(|v| v.as_array()).map(|a| a.len()),
			Some(0)
		);

		let grouped: Value = private
			.post("/api/versions/get_grouped_versions")
			.json(&serde_json::json!({}))
			.await
			.json();
		let group = &grouped.as_array().unwrap()[0];
		let v = &group["versions"].as_array().unwrap()[0];
		assert_eq!(v.get("ready"), Some(&Value::Bool(true)));

		// version_id is referenced from the database state, drop the local
		// binding so the compiler doesn't flag it as unused on test paths
		// that don't query by it directly.
		let _ = version_id;
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn add_open_issue_makes_version_not_ready() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_version(&mut conn).await;

		let resp = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": version_id,
				"description": "Reports filter fails on Postgres 17",
			}))
			.await;
		resp.assert_status_ok();

		let detail: Value = private
			.post("/api/versions/get_version_detail")
			.json(&serde_json::json!({ "version": "1.0.0" }))
			.await
			.json();
		assert_eq!(detail.get("ready").and_then(|v| v.as_bool()), Some(false));
		let issues = detail["known_issues"].as_array().unwrap();
		assert_eq!(issues.len(), 1);
		assert_eq!(
			issues[0]["description"],
			"Reports filter fails on Postgres 17"
		);
		assert!(issues[0]["resolved_at"].is_null());

		let grouped: Value = private
			.post("/api/versions/get_grouped_versions")
			.json(&serde_json::json!({}))
			.await
			.json();
		let v = &grouped.as_array().unwrap()[0]["versions"].as_array().unwrap()[0];
		assert_eq!(v.get("ready"), Some(&Value::Bool(false)));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_the_issue_restores_ready() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_version(&mut conn).await;

		let added: Value = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": version_id,
				"description": "Slow startup on cold boot",
			}))
			.await
			.json();
		let known_issue_id = added["id"].as_str().unwrap().to_string();

		let resolve_resp = private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": known_issue_id,
				"resolution_message": "Fixed in 1.0.1",
			}))
			.await;
		resolve_resp.assert_status_ok();
		let resolved: Value = resolve_resp.json();
		assert_eq!(resolved["resolution_message"], "Fixed in 1.0.1");
		assert!(!resolved["resolved_at"].is_null());

		let detail: Value = private
			.post("/api/versions/get_version_detail")
			.json(&serde_json::json!({ "version": "1.0.0" }))
			.await
			.json();
		assert_eq!(detail.get("ready").and_then(|v| v.as_bool()), Some(true));
		let issues = detail["known_issues"].as_array().unwrap();
		assert_eq!(issues.len(), 1);
		assert_eq!(issues[0]["resolution_message"], "Fixed in 1.0.1");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cannot_resolve_twice() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_version(&mut conn).await;
		let added: Value = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": version_id,
				"description": "x",
			}))
			.await
			.json();
		let known_issue_id = added["id"].as_str().unwrap().to_string();
		private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": known_issue_id,
				"resolution_message": "fixed",
			}))
			.await
			.assert_status_ok();
		// Second resolve hits the NOT FOUND path because the filter
		// `resolved_at IS NULL` excludes already-resolved rows.
		let resp = private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": known_issue_id,
				"resolution_message": "again",
			}))
			.await;
		assert_eq!(resp.status_code(), 404);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn list_known_issues_returns_all_for_version() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_version(&mut conn).await;
		for desc in ["a", "b", "c"] {
			private
				.post("/api/versions/add_known_issue")
				.json(&serde_json::json!({
					"version_id": version_id,
					"description": desc,
				}))
				.await
				.assert_status_ok();
		}
		let resp = private
			.post("/api/versions/list_known_issues")
			.json(&serde_json::json!({ "version_id": version_id }))
			.await;
		resp.assert_status_ok();
		let issues: Vec<Value> = resp.json();
		assert_eq!(issues.len(), 3);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn add_rejects_empty_description() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_version(&mut conn).await;
		let resp = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": version_id,
				"description": "   ",
			}))
			.await;
		assert_eq!(resp.status_code(), 400);
	})
	.await;
}
