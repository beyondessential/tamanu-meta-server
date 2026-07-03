//! Endpoint tests for the operator-facing `/api/restore_replicas/*` fns —
//! parameter validation against the consumer's advertised schema, and that
//! declared parameter values round-trip back through the group view.

use commons_tests::diesel_async::{AsyncPgConnection, SimpleAsyncConnection};
use commons_types::backup::IntentDescriptor;
use database::RestoreConsumerCapability;
use uuid::Uuid;

async fn insert_group(conn: &mut AsyncPgConnection) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{id}', 'rr-test')"
	))
	.await
	.expect("insert group");
	id
}

async fn insert_consumer(conn: &mut AsyncPgConnection) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO devices (id, role) VALUES ('{id}', 'backup-restore')"
	))
	.await
	.expect("insert device");
	id
}

/// Advertise an `analytics` intent accepting a boolean `anonymisation` param.
async fn advertise_analytics(conn: &mut AsyncPgConnection, consumer: Uuid) {
	let descriptor: IntentDescriptor = serde_json::from_value(serde_json::json!({
		"intent": "analytics",
		"semantics": ["check", "url"],
		"params": { "anonymisation": {"type": "boolean", "default": true} },
	}))
	.unwrap();
	RestoreConsumerCapability::register(conn, consumer, &[descriptor])
		.await
		.expect("register caps");
}

#[tokio::test(flavor = "multi_thread")]
async fn create_rejects_wrong_typed_param() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group = insert_group(&mut conn).await;
		let consumer = insert_consumer(&mut conn).await;
		advertise_analytics(&mut conn, consumer).await;

		// `anonymisation` is a boolean; a string value is rejected as 400.
		private
			.post("/api/restore_replicas/create")
			.json(&serde_json::json!({
				"consumer_device_id": consumer,
				"group_id": group,
				"server_id": null,
				"type": "tamanu-postgres",
				"intent": "analytics",
				"name": "an-decl",
				"overdue_after_seconds": null,
				"params": { "anonymisation": "yes" },
			}))
			.await
			.assert_status_bad_request();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn create_stores_valid_params_and_they_round_trip() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group = insert_group(&mut conn).await;
		let consumer = insert_consumer(&mut conn).await;
		advertise_analytics(&mut conn, consumer).await;

		private
			.post("/api/restore_replicas/create")
			.json(&serde_json::json!({
				"consumer_device_id": consumer,
				"group_id": group,
				"server_id": null,
				"type": "tamanu-postgres",
				"intent": "analytics",
				"name": "an-decl",
				"overdue_after_seconds": 7200,
				"params": { "anonymisation": false },
			}))
			.await
			.assert_status_ok();

		let resp = private
			.post("/api/restore_replicas/for_group")
			.json(&serde_json::json!({ "server_group_id": group }))
			.await;
		resp.assert_status_ok();
		let rows: Vec<serde_json::Value> = resp.json();
		assert_eq!(rows.len(), 1, "got {rows:?}");
		assert_eq!(rows[0]["overdue_after_seconds"], 7200);
		assert_eq!(rows[0]["params"]["anonymisation"], false);
		assert_eq!(rows[0]["gap"], false, "advertised intent is not a gap");
	})
	.await;
}
