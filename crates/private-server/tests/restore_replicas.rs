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

async fn insert_server(conn: &mut AsyncPgConnection, group_id: Uuid) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO servers (id, name, host, kind, rank, group_id) VALUES
			('{id}', 'rr-test-server', 'https://{id}.example.com', 'facility',
			 'production', '{group_id}')"
	))
	.await
	.expect("insert server");
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

/// Advertise a `verify` intent with no parameters.
async fn advertise_verify(conn: &mut AsyncPgConnection, consumer: Uuid) {
	let descriptor: IntentDescriptor = serde_json::from_value(serde_json::json!({
		"intent": "verify",
		"semantics": ["check"],
		"params": {},
	}))
	.unwrap();
	RestoreConsumerCapability::register(conn, consumer, &[descriptor])
		.await
		.expect("register caps");
}

/// Advertise both `verify` (no params) and `analytics` (boolean `anonymisation`)
/// on the same consumer, so a declaration can be retargeted between them.
async fn advertise_verify_and_analytics(conn: &mut AsyncPgConnection, consumer: Uuid) {
	let descriptors: Vec<IntentDescriptor> = serde_json::from_value(serde_json::json!([
		{ "intent": "verify", "semantics": ["check"], "params": {} },
		{
			"intent": "analytics",
			"semantics": ["check", "url"],
			"params": { "anonymisation": {"type": "boolean", "default": true} },
		},
	]))
	.unwrap();
	RestoreConsumerCapability::register(conn, consumer, &descriptors)
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

/// Declare a replica via the HTTP endpoint and return its id.
async fn create_replica(
	private: &commons_tests::axum_test::TestServer,
	consumer: Uuid,
	group: Uuid,
	intent: &str,
	params: serde_json::Value,
) -> Uuid {
	let resp = private
		.post("/api/restore_replicas/create")
		.json(&serde_json::json!({
			"consumer_device_id": consumer,
			"group_id": group,
			"server_id": null,
			"type": "tamanu-postgres",
			"intent": intent,
			"name": format!("{intent}-decl"),
			"overdue_after_seconds": null,
			"params": params,
		}))
		.await;
	resp.assert_status_ok();
	let body: serde_json::Value = resp.json();
	body["id"].as_str().unwrap().parse().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn update_changes_intent_and_round_trips() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group = insert_group(&mut conn).await;
		let consumer = insert_consumer(&mut conn).await;
		advertise_verify_and_analytics(&mut conn, consumer).await;

		let id = create_replica(&private, consumer, group, "verify", serde_json::json!({})).await;

		private
			.post("/api/restore_replicas/update")
			.json(&serde_json::json!({
				"id": id,
				"consumer_device_id": consumer,
				"group_id": group,
				"server_id": null,
				"type": "tamanu-postgres",
				"intent": "analytics",
				"name": "verify-decl",
				"overdue_after_seconds": null,
				"params": { "anonymisation": true },
				"enabled": true,
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
		assert_eq!(rows[0]["intent"], "analytics");
		assert_eq!(rows[0]["params"]["anonymisation"], true);
		assert_eq!(rows[0]["gap"], false);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_scope_collision_conflicts() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group = insert_group(&mut conn).await;
		let consumer = insert_consumer(&mut conn).await;
		advertise_verify_and_analytics(&mut conn, consumer).await;

		create_replica(&private, consumer, group, "verify", serde_json::json!({})).await;
		let b = create_replica(
			&private,
			consumer,
			group,
			"analytics",
			serde_json::json!({}),
		)
		.await;

		// Retargeting b's intent onto a's (consumer, group, type, intent, server)
		// scope collides.
		private
			.post("/api/restore_replicas/update")
			.json(&serde_json::json!({
				"id": b,
				"consumer_device_id": consumer,
				"group_id": group,
				"server_id": null,
				"type": "tamanu-postgres",
				"intent": "verify",
				"name": "analytics-decl",
				"overdue_after_seconds": null,
				"params": {},
				"enabled": true,
			}))
			.await
			.assert_status_conflict();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_revalidates_params_against_new_intent() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group = insert_group(&mut conn).await;
		let consumer = insert_consumer(&mut conn).await;
		advertise_verify_and_analytics(&mut conn, consumer).await;

		let id = create_replica(&private, consumer, group, "verify", serde_json::json!({})).await;

		// `anonymisation` is a boolean; a string value fails validation against
		// analytics' schema.
		private
			.post("/api/restore_replicas/update")
			.json(&serde_json::json!({
				"id": id,
				"consumer_device_id": consumer,
				"group_id": group,
				"server_id": null,
				"type": "tamanu-postgres",
				"intent": "analytics",
				"name": "verify-decl",
				"overdue_after_seconds": null,
				"params": { "anonymisation": "yes" },
				"enabled": true,
			}))
			.await
			.assert_status_bad_request();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_can_change_consumer_and_server_scope() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group = insert_group(&mut conn).await;
		let server = insert_server(&mut conn, group).await;
		let consumer_a = insert_consumer(&mut conn).await;
		let consumer_b = insert_consumer(&mut conn).await;
		advertise_verify(&mut conn, consumer_a).await;
		advertise_verify(&mut conn, consumer_b).await;

		let id = create_replica(&private, consumer_a, group, "verify", serde_json::json!({})).await;

		// Reassign the declaration to a different consumer and narrow it from
		// "every server in the group" to one specific server.
		private
			.post("/api/restore_replicas/update")
			.json(&serde_json::json!({
				"id": id,
				"consumer_device_id": consumer_b,
				"group_id": group,
				"server_id": server,
				"type": "tamanu-postgres",
				"intent": "verify",
				"name": "verify-decl",
				"overdue_after_seconds": null,
				"params": {},
				"enabled": true,
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
		assert_eq!(rows[0]["consumer_device_id"], consumer_b.to_string());
		assert_eq!(rows[0]["server_id"], server.to_string());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_to_unadvertised_intent_creates_gap() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group = insert_group(&mut conn).await;
		let consumer = insert_consumer(&mut conn).await;
		// This consumer only advertises `verify`.
		advertise_verify(&mut conn, consumer).await;

		let id = create_replica(&private, consumer, group, "verify", serde_json::json!({})).await;

		// Retargeting to `analytics`, which the consumer doesn't advertise, is
		// accepted with the params passing through unvalidated — same as create,
		// which allows declaring ahead of the consumer registering support. The
		// declaration surfaces as a gap.
		private
			.post("/api/restore_replicas/update")
			.json(&serde_json::json!({
				"id": id,
				"consumer_device_id": consumer,
				"group_id": group,
				"server_id": null,
				"type": "tamanu-postgres",
				"intent": "analytics",
				"name": "verify-decl",
				"overdue_after_seconds": null,
				"params": { "not_in_any_schema": "kept-as-is" },
				"enabled": true,
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
		assert_eq!(rows[0]["intent"], "analytics");
		assert_eq!(rows[0]["gap"], true, "unadvertised intent is a gap");
		assert_eq!(rows[0]["params"]["not_in_any_schema"], "kept-as-is");
	})
	.await;
}
