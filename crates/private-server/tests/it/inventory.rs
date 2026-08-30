//! Endpoint tests for `/api/inventory/for_group` — what a configuration run
//! reads for one deployment, and the refusals it has to tell apart from
//! Canopy being unreachable.
//!
//! spec: INV

use commons_tests::diesel_async::{AsyncPgConnection, SimpleAsyncConnection};
use serde_json::{Value, json};
use uuid::Uuid;

async fn insert_group(conn: &mut AsyncPgConnection, name: &str, tags: Value) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name, tags) VALUES ('{id}', '{name}', '{tags}')"
	))
	.await
	.expect("insert group");
	id
}

async fn insert_server(
	conn: &mut AsyncPgConnection,
	group: Uuid,
	name: &str,
	kind: &str,
	host: Option<&str>,
	tags: Value,
) -> Uuid {
	let id = Uuid::new_v4();
	let host = host.map_or("NULL".to_string(), |h| format!("'{h}'"));
	conn.batch_execute(&format!(
		"INSERT INTO servers (id, name, kind, host, group_id, tags)
		 VALUES ('{id}', '{name}', '{kind}', {host}, '{group}', '{tags}')"
	))
	.await
	.expect("insert server");
	id
}

async fn bind_device(conn: &mut AsyncPgConnection, server: Uuid, tailscale_name: &str) {
	let device = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO devices (id, role, tailscale_node_name)
		 VALUES ('{device}', 'server', '{tailscale_name}');
		 UPDATE servers SET device_id = '{device}' WHERE id = '{server}'"
	))
	.await
	.expect("bind device");
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_a_deployments_hosts_and_variables() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(
			&mut conn,
			"kamaka-prod",
			json!({
				"timezone": "Pacific/Auckland",
				"tamanu_version": "v2.54.8",
				"elastic_agent_enabled": "false",
			}),
		)
		.await;
		let central = insert_server(
			&mut conn,
			group,
			"kamaka-prod-central",
			"central",
			None,
			json!({
				"public_hostname": "central.kamaka.example",
				"elastic_agent_enabled": "true",
				"tamanu_caddy_extra_hostnames": "[\"sync.kamaka.example\"]",
				"ansible_port": "2222",
			}),
		)
		.await;
		bind_device(&mut conn, central, "kamaka-prod-central").await;
		insert_server(
			&mut conn,
			group,
			"kamaka-prod-facility",
			"facility",
			Some("https://facility.kamaka.example/"),
			json!({ "canopy:kind": "spoofed" }),
		)
		.await;

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-prod" }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();

		assert_eq!(body["group"], "kamaka-prod");
		assert_eq!(body["vars"]["timezone"], "Pacific/Auckland");
		assert_eq!(body["vars"]["elastic_agent_enabled"], json!(false));

		let hosts = body["hosts"].as_array().expect("hosts");
		assert_eq!(hosts.len(), 2);

		// A bound device's tailnet name is the address, and the group's values
		// carry down except where the host sets its own.
		assert_eq!(hosts[0]["name"], "kamaka-prod-central");
		assert_eq!(hosts[0]["kind"], "central");
		assert_eq!(hosts[0]["product"], "tamanu");
		assert_eq!(hosts[0]["address"], "kamaka-prod-central");
		assert_eq!(hosts[0]["vars"]["timezone"], "Pacific/Auckland");
		assert_eq!(hosts[0]["vars"]["elastic_agent_enabled"], json!(true));
		assert_eq!(
			hosts[0]["vars"]["tamanu_caddy_extra_hostnames"],
			json!(["sync.kamaka.example"])
		);
		assert_eq!(hosts[0]["vars"]["ansible_port"], "2222");

		// No device, so the address is the recorded host as a bare name, and a
		// stored tag in the reserved namespace is not served as a variable.
		assert_eq!(hosts[1]["address"], "facility.kamaka.example");
		assert!(hosts[1]["vars"].get("canopy:kind").is_none());
		assert_eq!(hosts[1]["vars"]["timezone"], "Pacific/Auckland");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_a_deployment_asked_for_by_identifier() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "drifting-demo", json!({})).await;
		insert_server(
			&mut conn,
			group,
			"drifting-demo-central",
			"central",
			None,
			json!({}),
		)
		.await;

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "server_group_id": group }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();
		assert_eq!(body["group"], "drifting-demo");
		assert_eq!(body["hosts"].as_array().expect("hosts").len(), 1);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_group_canopy_does_not_have() {
	commons_tests::server::run(async move |_conn, _public, private| {
		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "nowhere-prod" }))
			.await;
		response.assert_status_not_found();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_an_archived_group() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka-clone", json!({})).await;
		insert_server(
			&mut conn,
			group,
			"kamaka-clone-central",
			"central",
			None,
			json!({}),
		)
		.await;
		conn.batch_execute(&format!(
			"UPDATE server_groups SET deleted_at = now() WHERE id = '{group}'"
		))
		.await
		.expect("archive group");

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-clone" }))
			.await;
		response.assert_status_conflict();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_group_with_nothing_to_configure() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		insert_group(&mut conn, "kamaka-demo", json!({})).await;

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-demo" }))
			.await;
		response.assert_status_conflict();
	})
	.await
}

/// Names aren't unique, and serving one of two deployments that answer to the
/// same name would configure the wrong fleet.
#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_name_that_answers_for_two_groups() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		for _ in 0..2 {
			let group = insert_group(&mut conn, "twice-prod", json!({})).await;
			insert_server(
				&mut conn,
				group,
				"twice-prod-central",
				"central",
				None,
				json!({}),
			)
			.await;
		}

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "twice-prod" }))
			.await;
		response.assert_status_conflict();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_request_naming_neither_a_group_nor_an_identifier() {
	commons_tests::server::run(async move |_conn, _public, private| {
		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({}))
			.await;
		response.assert_status_bad_request();
	})
	.await
}
