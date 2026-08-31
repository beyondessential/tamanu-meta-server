//! Endpoint tests for `/api/inventory/for_group` — what a configuration run
//! reads for one environment, and the refusals it has to tell apart from
//! Canopy being unreachable.
//!
//! spec: INV

use commons_tests::diesel_async::{AsyncPgConnection, SimpleAsyncConnection};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) async fn insert_group(conn: &mut AsyncPgConnection, name: &str, tags: Value) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name, tags) VALUES ('{id}', '{name}', '{tags}')"
	))
	.await
	.expect("insert group");
	id
}

pub(crate) async fn insert_server(
	conn: &mut AsyncPgConnection,
	group: Uuid,
	name: &str,
	kind: &str,
	host: Option<&str>,
	tags: Value,
) -> Uuid {
	insert_ranked_server(conn, group, name, kind, None, host, tags).await
}

pub(crate) async fn insert_ranked_server(
	conn: &mut AsyncPgConnection,
	group: Uuid,
	name: &str,
	kind: &str,
	rank: Option<&str>,
	host: Option<&str>,
	tags: Value,
) -> Uuid {
	let id = Uuid::new_v4();
	let host = host.map_or("NULL".to_string(), |h| format!("'{h}'"));
	let rank = rank.map_or("NULL".to_string(), |r| format!("'{r}'"));
	conn.batch_execute(&format!(
		"INSERT INTO servers (id, name, kind, rank, host, group_id, tags)
		 VALUES ('{id}', '{name}', '{kind}', {rank}, {host}, '{group}', '{tags}')"
	))
	.await
	.expect("insert server");
	id
}

pub(crate) async fn bind_device(conn: &mut AsyncPgConnection, server: Uuid, tailscale_name: &str) {
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
async fn serves_an_environments_servers_and_variables() {
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
		// Nothing here carries a rank, so the whole group is one environment at
		// the default rank.
		assert_eq!(body["rank"], "dev");
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

		// What the server sets itself is served apart from what it inherits, so
		// a value can be traced to where it is set.
		assert_eq!(hosts[0]["own_vars"]["elastic_agent_enabled"], json!(true));
		assert!(hosts[0]["own_vars"].get("timezone").is_none());

		// No device, so the address is the recorded host as a bare name, and a
		// stored tag in the reserved namespace is not served as a variable.
		assert_eq!(hosts[1]["address"], "facility.kamaka.example");
		assert!(hosts[1]["vars"].get("canopy:kind").is_none());
		assert_eq!(hosts[1]["vars"]["timezone"], "Pacific/Auckland");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_the_environment_at_the_rank_asked_for() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka", json!({})).await;
		insert_ranked_server(
			&mut conn,
			group,
			"kamaka-prod-central",
			"central",
			Some("production"),
			None,
			json!({}),
		)
		.await;
		insert_ranked_server(
			&mut conn,
			group,
			"kamaka-demo-central",
			"central",
			Some("demo"),
			None,
			json!({}),
		)
		.await;

		// A group spanning two environments cannot be served as one: a run
		// configuring the demo servers alongside the production ones is never
		// what was meant.
		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka" }))
			.await;
		response.assert_status_conflict();

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka", "rank": "production" }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();
		assert_eq!(body["rank"], "production");
		let hosts = body["hosts"].as_array().expect("hosts");
		assert_eq!(hosts.len(), 1);
		assert_eq!(hosts[0]["name"], "kamaka-prod-central");

		// A rank the group holds no live server at is refused, rather than
		// answered with an empty inventory.
		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka", "rank": "test" }))
			.await;
		response.assert_status_conflict();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_an_environment_asked_for_by_identifier() {
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

/// The address falls back to the recorded host when the bound device has no
/// tailnet name of its own.
#[tokio::test(flavor = "multi_thread")]
async fn falls_back_to_the_recorded_host_for_a_nameless_device() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka-nameless", json!({})).await;
		let server = insert_server(
			&mut conn,
			group,
			"kamaka-nameless-central",
			"central",
			Some("https://central.kamaka.example/"),
			json!({}),
		)
		.await;
		let device = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role) VALUES ('{device}', 'server');
			 UPDATE servers SET device_id = '{device}' WHERE id = '{server}'"
		))
		.await
		.expect("bind device");

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-nameless" }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();
		assert_eq!(body["hosts"][0]["address"], "central.kamaka.example");
	})
	.await
}

/// An archived server is not something to configure, so it leaves the
/// inventory without the group leaving it.
#[tokio::test(flavor = "multi_thread")]
async fn leaves_out_an_archived_server() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka-archived", json!({})).await;
		insert_server(
			&mut conn,
			group,
			"kamaka-archived-central",
			"central",
			None,
			json!({}),
		)
		.await;
		let gone = insert_server(
			&mut conn,
			group,
			"kamaka-archived-facility",
			"facility",
			None,
			json!({}),
		)
		.await;
		conn.batch_execute(&format!(
			"UPDATE servers SET deleted_at = now() WHERE id = '{gone}'"
		))
		.await
		.expect("archive server");

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-archived" }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();
		let hosts = body["hosts"].as_array().expect("hosts");
		assert_eq!(hosts.len(), 1);
		assert_eq!(hosts[0]["name"], "kamaka-archived-central");
	})
	.await
}

/// A server in no group is in no inventory: it has no group whose tags would
/// configure it and no environment to belong to.
#[tokio::test(flavor = "multi_thread")]
async fn leaves_out_a_server_belonging_to_no_group() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka-lonely", json!({})).await;
		insert_server(
			&mut conn,
			group,
			"kamaka-lonely-central",
			"central",
			None,
			json!({}),
		)
		.await;
		let loose = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, name, kind) VALUES ('{loose}', 'kamaka-loose', 'central')"
		))
		.await
		.expect("insert ungrouped server");

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-lonely" }))
			.await;
		response.assert_status_ok();
		let hosts = response.json::<Value>()["hosts"]
			.as_array()
			.expect("hosts")
			.clone();
		assert_eq!(hosts.len(), 1);
		assert_eq!(hosts[0]["name"], "kamaka-lonely-central");
	})
	.await
}

/// Canopy's own placeholder server is not something to configure, and its
/// recorded host is a loopback address a run would take literally.
#[tokio::test(flavor = "multi_thread")]
async fn leaves_out_the_meta_server() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka-meta", json!({})).await;
		insert_server(
			&mut conn,
			group,
			"kamaka-meta-central",
			"central",
			None,
			json!({}),
		)
		.await;
		conn.batch_execute(&format!(
			"UPDATE servers SET group_id = '{group}' WHERE id = '{}'",
			Uuid::nil()
		))
		.await
		.expect("group the meta server");

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-meta" }))
			.await;
		response.assert_status_ok();
		let hosts = response.json::<Value>()["hosts"]
			.as_array()
			.expect("hosts")
			.clone();
		assert_eq!(hosts.len(), 1);
		assert_eq!(hosts[0]["name"], "kamaka-meta-central");
	})
	.await
}

/// A tag that opens like JSON and isn't stays the text it was stored as,
/// rather than becoming an error or a half-parsed value.
#[tokio::test(flavor = "multi_thread")]
async fn serves_a_malformed_json_tag_as_text() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka-malformed", json!({})).await;
		insert_server(
			&mut conn,
			group,
			"kamaka-malformed-central",
			"central",
			None,
			json!({ "tamanu_caddy_extra_hostnames": "[\"sync.kamaka.example\"" }),
		)
		.await;

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-malformed" }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();
		assert_eq!(
			body["hosts"][0]["vars"]["tamanu_caddy_extra_hostnames"],
			"[\"sync.kamaka.example\""
		);
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

/// Names aren't unique, and serving one of two groups that answer to the same
/// name would configure the wrong fleet.
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

/// Both together are as ambiguous as neither: canopy picks nothing.
#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_request_naming_both_a_group_and_an_identifier() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka-both", json!({})).await;
		insert_server(
			&mut conn,
			group,
			"kamaka-both-central",
			"central",
			None,
			json!({}),
		)
		.await;

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "group": "kamaka-both", "server_group_id": group }))
			.await;
		response.assert_status_bad_request();
	})
	.await
}
