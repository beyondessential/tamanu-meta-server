//! Endpoints for manual incidents at `/api/manual_incidents`: list with its
//! filters, get by id, the UI's create/update/delete (attributed to the
//! tailnet user), validation failures, and 404s on unknown ids.

use commons_tests::diesel_async::SimpleAsyncConnection;
use database::manual_incidents::ManualIncident;
use jiff::Timestamp;
use uuid::Uuid;

fn ts(s: &str) -> Timestamp {
	s.parse().unwrap()
}

async fn seed_groups(conn: &mut commons_tests::diesel_async::AsyncPgConnection) -> (Uuid, Uuid) {
	let group_id = Uuid::new_v4();
	let other_id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES \
			('{group_id}', 'Manual Group'), ('{other_id}', 'Other Group')"
	))
	.await
	.expect("seed groups");
	(group_id, other_id)
}

async fn seed(
	conn: &mut commons_tests::diesel_async::AsyncPgConnection,
) -> (Uuid, ManualIncident, ManualIncident) {
	let (group_id, other_id) = seed_groups(conn).await;

	let older = ManualIncident::create(
		&mut *conn,
		"Older ended incident",
		"",
		ts("2026-07-01T10:00:00Z"),
		Some(ts("2026-07-01T12:00:00Z")),
		other_id,
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
		group_id,
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
		assert_eq!(items[1]["server_group_name"], "Other Group");
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

#[tokio::test(flavor = "multi_thread")]
async fn create_update_delete_roundtrip_with_attribution() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let (group_id, other_id) = seed_groups(&mut conn).await;

		// Create: attributed to the tailnet user (dev bypass = admin@localhost).
		let resp = private
			.post("/api/manual_incidents/create")
			.json(&serde_json::json!({
				"title": "  Fibre cut in Suva  ",
				"description": "ISP outage.",
				"startedAt": "2026-07-01T10:00:00Z",
				"serverGroupId": group_id,
			}))
			.await;
		resp.assert_status_ok();
		let created: serde_json::Value = resp.json();
		assert_eq!(created["title"], "Fibre cut in Suva", "title is trimmed");
		assert_eq!(created["server_group_id"], group_id.to_string());
		assert_eq!(created["server_group_name"], "Manual Group");
		assert_eq!(created["created_by"], "admin@localhost");
		assert!(created["ended_at"].is_null());
		let id = created["id"].as_str().expect("id").to_string();

		// Update: end it and move it to the other group in one edit.
		let resp = private
			.post("/api/manual_incidents/update")
			.json(&serde_json::json!({
				"id": id,
				"endedAt": "2026-07-01T12:30:00Z",
				"serverGroupId": other_id,
			}))
			.await;
		resp.assert_status_ok();
		let updated: serde_json::Value = resp.json();
		assert!(!updated["ended_at"].is_null());
		assert_eq!(updated["server_group_id"], other_id.to_string());
		assert_eq!(updated["server_group_name"], "Other Group");
		assert_eq!(updated["title"], "Fibre cut in Suva", "title unchanged");

		// clearEndedAt marks it ongoing again.
		let resp = private
			.post("/api/manual_incidents/update")
			.json(&serde_json::json!({ "id": id, "clearEndedAt": true }))
			.await;
		resp.assert_status_ok();
		let cleared: serde_json::Value = resp.json();
		assert!(cleared["ended_at"].is_null());

		// Delete removes it; the detail endpoint 404s afterwards.
		let resp = private
			.post("/api/manual_incidents/delete")
			.json(&serde_json::json!({ "id": id }))
			.await;
		resp.assert_status_ok();
		let gone = private
			.post("/api/manual_incidents/get")
			.json(&serde_json::json!({ "id": id }))
			.await;
		assert_eq!(gone.status_code().as_u16(), 404);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_validate_input_and_404_unknown_ids() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let (group_id, _other) = seed_groups(&mut conn).await;

		// Blank title.
		let resp = private
			.post("/api/manual_incidents/create")
			.json(&serde_json::json!({
				"title": "   ",
				"startedAt": "2026-07-01T10:00:00Z",
				"serverGroupId": group_id,
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 400);

		// Unknown group.
		let resp = private
			.post("/api/manual_incidents/create")
			.json(&serde_json::json!({
				"title": "orphan",
				"startedAt": "2026-07-01T10:00:00Z",
				"serverGroupId": Uuid::new_v4(),
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 400);

		// Missing group: the field is required.
		let resp = private
			.post("/api/manual_incidents/create")
			.json(&serde_json::json!({
				"title": "groupless",
				"startedAt": "2026-07-01T10:00:00Z",
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 422);

		let incident = ManualIncident::create(
			&mut conn,
			"target",
			"",
			ts("2026-07-01T10:00:00Z"),
			None,
			group_id,
			"t",
		)
		.await
		.expect("create");

		// Conflicting end-time edits.
		let resp = private
			.post("/api/manual_incidents/update")
			.json(&serde_json::json!({
				"id": incident.id,
				"endedAt": "2026-07-01T12:00:00Z",
				"clearEndedAt": true,
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 400);

		// Blank title on update.
		let resp = private
			.post("/api/manual_incidents/update")
			.json(&serde_json::json!({ "id": incident.id, "title": " " }))
			.await;
		assert_eq!(resp.status_code().as_u16(), 400);

		// Moving to an unknown group.
		let resp = private
			.post("/api/manual_incidents/update")
			.json(&serde_json::json!({ "id": incident.id, "serverGroupId": Uuid::new_v4() }))
			.await;
		assert_eq!(resp.status_code().as_u16(), 400);

		// Unknown ids 404.
		let resp = private
			.post("/api/manual_incidents/update")
			.json(&serde_json::json!({ "id": Uuid::new_v4(), "title": "x" }))
			.await;
		assert_eq!(resp.status_code().as_u16(), 404);
		let resp = private
			.post("/api/manual_incidents/delete")
			.json(&serde_json::json!({ "id": Uuid::new_v4() }))
			.await;
		assert_eq!(resp.status_code().as_u16(), 404);
	})
	.await
}
