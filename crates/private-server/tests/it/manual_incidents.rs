//! Read endpoints for manual incidents at `/api/manual_incidents`: list with
//! its filters (rows seeded directly through the model — writes happen over
//! MCP, not these endpoints), get by id, and 404 on unknown ids.

use commons_tests::diesel_async::SimpleAsyncConnection;
use database::manual_incidents::ManualIncident;
use jiff::Timestamp;
use uuid::Uuid;

fn ts(s: &str) -> Timestamp {
	s.parse().unwrap()
}

async fn seed(
	conn: &mut commons_tests::diesel_async::AsyncPgConnection,
) -> (Uuid, ManualIncident, ManualIncident) {
	let group_id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'Manual Group')"
	))
	.await
	.expect("seed group");

	let older = ManualIncident::create(
		&mut *conn,
		"Older ended incident",
		"",
		ts("2026-07-01T10:00:00Z"),
		Some(ts("2026-07-01T12:00:00Z")),
		None,
		"scribe-token",
	)
	.await
	.expect("create older");
	let newer = ManualIncident::create(
		&mut *conn,
		"Newer ongoing incident",
		"Still being worked on.",
		ts("2026-07-02T10:00:00Z"),
		None,
		Some(group_id),
		"admin@localhost",
	)
	.await
	.expect("create newer");
	(group_id, older, newer)
}

#[tokio::test(flavor = "multi_thread")]
async fn list_returns_seeded_rows_with_filters() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let (group_id, older, newer) = seed(&mut conn).await;

		let resp = private
			.post("/api/manual_incidents/list")
			.json(&serde_json::json!({}))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 2);
		// Most recently started first.
		assert_eq!(items[0]["id"], newer.id.to_string());
		assert_eq!(items[0]["title"], "Newer ongoing incident");
		assert_eq!(items[0]["description"], "Still being worked on.");
		assert!(items[0]["ended_at"].is_null());
		assert_eq!(items[0]["server_group_id"], group_id.to_string());
		assert_eq!(items[0]["server_group_name"], "Manual Group");
		assert_eq!(items[0]["created_by"], "admin@localhost");
		assert_eq!(items[1]["id"], older.id.to_string());
		assert!(!items[1]["ended_at"].is_null());
		assert!(items[1]["server_group_id"].is_null());
		assert!(items[1]["server_group_name"].is_null());
		assert_eq!(items[1]["created_by"], "scribe-token");

		// Group filter.
		let resp = private
			.post("/api/manual_incidents/list")
			.json(&serde_json::json!({ "groupId": group_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert_eq!(items[0]["id"], newer.id.to_string());

		// Ongoing-only filter drops the ended one.
		let resp = private
			.post("/api/manual_incidents/list")
			.json(&serde_json::json!({ "ongoingOnly": true }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert_eq!(items[0]["id"], newer.id.to_string());

		// Limit truncates after ordering.
		let resp = private
			.post("/api/manual_incidents/list")
			.json(&serde_json::json!({ "limit": 1 }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert_eq!(items[0]["id"], newer.id.to_string());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_returns_one_and_404s_on_unknown_id() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let (group_id, _older, newer) = seed(&mut conn).await;

		let resp = private
			.post("/api/manual_incidents/get")
			.json(&serde_json::json!({ "id": newer.id }))
			.await;
		resp.assert_status_ok();
		let item: serde_json::Value = resp.json();
		assert_eq!(item["id"], newer.id.to_string());
		assert_eq!(item["title"], "Newer ongoing incident");
		assert_eq!(item["server_group_id"], group_id.to_string());
		assert_eq!(item["server_group_name"], "Manual Group");
		assert_eq!(item["created_by"], "admin@localhost");
		assert!(item["ended_at"].is_null());

		let missing = private
			.post("/api/manual_incidents/get")
			.json(&serde_json::json!({ "id": Uuid::new_v4() }))
			.await;
		assert_eq!(missing.status_code().as_u16(), 404);
	})
	.await
}
