use commons_tests::{diesel_async::SimpleAsyncConnection, server};
use serde_json::Value;

const V1_0_0: &str = "11111111-1111-1111-1111-111111111111";
const V1_0_1: &str = "11111111-1111-1111-1111-111111111101";
const V1_0_2: &str = "11111111-1111-1111-1111-111111111102";
const V1_0_3: &str = "11111111-1111-1111-1111-111111111103";

async fn seed_versions(conn: &mut commons_tests::diesel_async::AsyncPgConnection) {
	conn.batch_execute(&format!(
		"INSERT INTO versions (id, major, minor, patch, status, changelog) VALUES
		('{V1_0_0}', 1, 0, 0, 'published', 'Version 1.0.0'),
		('{V1_0_1}', 1, 0, 1, 'published', 'Version 1.0.1'),
		('{V1_0_2}', 1, 0, 2, 'published', 'Version 1.0.2'),
		('{V1_0_3}', 1, 0, 3, 'published', 'Version 1.0.3')",
	))
	.await
	.unwrap();
}

async fn seed_single(conn: &mut commons_tests::diesel_async::AsyncPgConnection) -> &'static str {
	conn.batch_execute(&format!(
		"INSERT INTO versions (id, major, minor, patch, status, changelog) VALUES
		('{V1_0_0}', 1, 0, 0, 'published', 'Version 1.0.0')",
	))
	.await
	.unwrap();
	V1_0_0
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_is_true_when_no_known_issues() {
	server::run(async |mut conn, _public, private| {
		let _ = seed_single(&mut conn).await;

		let detail: Value = private
			.post("/api/versions/get_version_detail")
			.json(&serde_json::json!({ "version": "1.0.0" }))
			.await
			.json();
		assert_eq!(detail.get("ready").and_then(|v| v.as_bool()), Some(true));
		assert_eq!(
			detail
				.get("known_issues")
				.and_then(|v| v.as_array())
				.map(|a| a.len()),
			Some(0)
		);

		let grouped: Value = private
			.post("/api/versions/get_grouped_versions")
			.json(&serde_json::json!({}))
			.await
			.json();
		let group = &grouped.as_array().unwrap()[0];
		assert_eq!(group.get("ready"), Some(&Value::Bool(true)));
		let v = &group["versions"].as_array().unwrap()[0];
		assert_eq!(v.get("ready"), Some(&Value::Bool(true)));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn add_open_issue_makes_version_not_ready() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_single(&mut conn).await;

		let resp = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": version_id,
				"description": "Reports filter fails on Postgres 17",
			}))
			.await;
		resp.assert_status_ok();
		let added: Value = resp.json();
		assert_eq!(added["min_major"], 1);
		assert_eq!(added["min_minor"], 0);
		assert_eq!(added["min_patch"], 0);
		assert!(added["max_major"].is_null());

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
		let group = &grouped.as_array().unwrap()[0];
		assert_eq!(group.get("ready"), Some(&Value::Bool(false)));
		let v = &group["versions"].as_array().unwrap()[0];
		assert_eq!(v.get("ready"), Some(&Value::Bool(false)));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_narrows_the_range() {
	server::run(async |mut conn, _public, private| {
		seed_versions(&mut conn).await;

		// Issue raised on 1.0.1 — covers 1.0.1, 1.0.2, 1.0.3 (open).
		let added: Value = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": V1_0_1,
				"description": "Slow startup on cold boot",
			}))
			.await
			.json();
		let known_issue_id = added["id"].as_str().unwrap().to_string();

		for ver in ["1.0.0", "1.0.1", "1.0.2", "1.0.3"] {
			let d: Value = private
				.post("/api/versions/get_version_detail")
				.json(&serde_json::json!({ "version": ver }))
				.await
				.json();
			let want = ver == "1.0.0";
			assert_eq!(
				d["ready"].as_bool(),
				Some(want),
				"open issue: {ver} ready={want}"
			);
		}

		// Fix in 1.0.3 — range becomes [1.0.1, 1.0.3).
		private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": known_issue_id,
				"fix_version": "1.0.3",
				"resolution_message": "Fixed in 1.0.3",
			}))
			.await
			.assert_status_ok();

		for (ver, want) in [
			("1.0.0", true),
			("1.0.1", false),
			("1.0.2", false),
			("1.0.3", true),
		] {
			let d: Value = private
				.post("/api/versions/get_version_detail")
				.json(&serde_json::json!({ "version": ver }))
				.await
				.json();
			assert_eq!(
				d["ready"].as_bool(),
				Some(want),
				"after resolve: {ver} ready={want}"
			);
		}
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn minor_ready_reflects_latest_patch() {
	server::run(async |mut conn, _public, private| {
		seed_versions(&mut conn).await;

		// Raise an issue on 1.0.1 then resolve it at 1.0.2 — 1.0.3 should
		// not be affected, so the minor is ready.
		let added: Value = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": V1_0_1,
				"description": "Issue on .1",
			}))
			.await
			.json();
		let id = added["id"].as_str().unwrap().to_string();
		private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": id,
				"fix_version": "1.0.2",
				"resolution_message": "fixed",
			}))
			.await
			.assert_status_ok();

		let grouped: Value = private
			.post("/api/versions/get_grouped_versions")
			.json(&serde_json::json!({}))
			.await
			.json();
		let group = &grouped.as_array().unwrap()[0];
		assert_eq!(
			group.get("ready"),
			Some(&Value::Bool(true)),
			"latest patch (1.0.3) is unaffected"
		);

		// Now raise an issue on 1.0.3 itself — the minor should flip to
		// not-ready.
		private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": V1_0_3,
				"description": "Issue on .3",
			}))
			.await
			.assert_status_ok();

		let grouped: Value = private
			.post("/api/versions/get_grouped_versions")
			.json(&serde_json::json!({}))
			.await
			.json();
		let group = &grouped.as_array().unwrap()[0];
		assert_eq!(group.get("ready"), Some(&Value::Bool(false)));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_rejects_invalid_fix_version() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_single(&mut conn).await;
		let added: Value = private
			.post("/api/versions/add_known_issue")
			.json(&serde_json::json!({
				"version_id": version_id,
				"description": "x",
			}))
			.await
			.json();
		let known_issue_id = added["id"].as_str().unwrap().to_string();

		// Fix below min — rejected.
		let resp = private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": known_issue_id,
				"fix_version": "1.0.0",
				"resolution_message": "no",
			}))
			.await;
		assert_eq!(resp.status_code(), 404);

		// Fix in different minor — rejected.
		let resp = private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": known_issue_id,
				"fix_version": "1.1.0",
				"resolution_message": "no",
			}))
			.await;
		assert_eq!(resp.status_code(), 404);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cannot_resolve_twice() {
	server::run(async |mut conn, _public, private| {
		let version_id = seed_single(&mut conn).await;
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
				"fix_version": "1.0.1",
				"resolution_message": "fixed",
			}))
			.await
			.assert_status_ok();
		// Second resolve hits NOT FOUND because the filter requires
		// max_major IS NULL.
		let resp = private
			.post("/api/versions/resolve_known_issue")
			.json(&serde_json::json!({
				"known_issue_id": known_issue_id,
				"fix_version": "1.0.2",
				"resolution_message": "again",
			}))
			.await;
		assert_eq!(resp.status_code(), 404);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn list_known_issues_returns_minor_history() {
	server::run(async |mut conn, _public, private| {
		seed_versions(&mut conn).await;
		// Raise issues on different patches in the same minor.
		for (vid, desc) in [(V1_0_0, "a"), (V1_0_2, "b"), (V1_0_3, "c")] {
			private
				.post("/api/versions/add_known_issue")
				.json(&serde_json::json!({
					"version_id": vid,
					"description": desc,
				}))
				.await
				.assert_status_ok();
		}
		// list_known_issues for any version in the minor returns all 3.
		let resp = private
			.post("/api/versions/list_known_issues")
			.json(&serde_json::json!({ "version_id": V1_0_1 }))
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
		let version_id = seed_single(&mut conn).await;
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
