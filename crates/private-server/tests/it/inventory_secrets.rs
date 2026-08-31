//! Endpoint tests for the secret variables an environment or a server carries:
//! that a value reaches a run, that a name is a tag or a secret and never both,
//! and that a value Canopy cannot produce refuses the whole inventory.
//!
//! spec: INV#secret-variables

use commons_tests::diesel_async::{AsyncPgConnection, SimpleAsyncConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::inventory::{insert_group, insert_ranked_server};

/// Record a name with no value behind it, which only a lost or half-written
/// secret store would produce.
async fn declare_without_value(conn: &mut AsyncPgConnection, group: Uuid, rank: &str, name: &str) {
	conn.batch_execute(&format!(
		"INSERT INTO inventory_secret_variables (server_group_id, rank, name)
		 VALUES ('{group}', '{rank}', '{name}')"
	))
	.await
	.expect("declare secret");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_environment_secret_reaches_every_host() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(
			&mut conn,
			"kamaka",
			json!({ "timezone": "Pacific/Auckland" }),
		)
		.await;
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
			"kamaka-prod-facility",
			"facility",
			Some("production"),
			None,
			json!({}),
		)
		.await;

		private
			.post("/api/inventory_secrets/set")
			.json(&json!({
				"server_group_id": group,
				"rank": "production",
				"name": "salt",
				"value": "pepper",
			}))
			.await
			.assert_status_ok();

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "server_group_id": group, "rank": "production" }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();

		assert_eq!(body["vars"]["salt"], "pepper");
		assert_eq!(body["secret_vars"], json!(["salt"]));
		let hosts = body["hosts"].as_array().expect("hosts");
		assert_eq!(hosts.len(), 2);
		for host in hosts {
			assert_eq!(host["vars"]["salt"], "pepper");
			assert_eq!(host["secret_vars"], json!(["salt"]));
			// It belongs to the environment, so it is not the server's own.
			assert!(host["own_vars"].get("salt").is_none());
		}
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_servers_secret_overlays_the_environments() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "kamaka", json!({})).await;
		let central = insert_ranked_server(
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
			"kamaka-prod-facility",
			"facility",
			Some("production"),
			None,
			json!({}),
		)
		.await;

		for args in [
			json!({ "server_group_id": group, "rank": "production", "name": "salt", "value": "shared" }),
			json!({ "server_id": central, "name": "salt", "value": "its-own" }),
		] {
			private
				.post("/api/inventory_secrets/set")
				.json(&args)
				.await
				.assert_status_ok();
		}

		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "server_group_id": group, "rank": "production" }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();
		let hosts = body["hosts"].as_array().expect("hosts");

		assert_eq!(hosts[0]["name"], "kamaka-prod-central");
		assert_eq!(hosts[0]["vars"]["salt"], "its-own");
		assert_eq!(hosts[0]["own_vars"]["salt"], "its-own");
		assert_eq!(hosts[1]["vars"]["salt"], "shared");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_with_no_value_refuses_the_inventory() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(
			&mut conn,
			"kamaka",
			json!({ "timezone": "Pacific/Auckland" }),
		)
		.await;
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
		declare_without_value(&mut conn, group, "production", "salt").await;

		// Serving the rest would hand a run a member that looks configured and
		// is missing a value.
		let response = private
			.post("/api/inventory/for_group")
			.json(&json!({ "server_group_id": group, "rank": "production" }))
			.await;
		response.assert_status(axum::http::StatusCode::BAD_GATEWAY);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_name_is_a_tag_or_a_secret_and_never_both() {
	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(
			&mut conn,
			"kamaka",
			json!({ "timezone": "Pacific/Auckland" }),
		)
		.await;
		let central = insert_ranked_server(
			&mut conn,
			group,
			"kamaka-prod-central",
			"central",
			Some("production"),
			None,
			json!({}),
		)
		.await;

		private
			.post("/api/inventory_secrets/set")
			.json(&json!({
				"server_group_id": group,
				"rank": "production",
				"name": "timezone",
				"value": "pepper",
			}))
			.await
			.assert_status_bad_request();

		private
			.post("/api/inventory_secrets/set")
			.json(&json!({
				"server_group_id": group,
				"rank": "production",
				"name": "salt",
				"value": "pepper",
			}))
			.await
			.assert_status_ok();

		// And the other way round: the tag write is the one refused.
		private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": central,
				"data": { "tags": { "salt": "in-the-clear" } },
			}))
			.await
			.assert_status_bad_request();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_names_are_listed_without_their_values() {
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
		private
			.post("/api/inventory_secrets/set")
			.json(&json!({
				"server_group_id": group,
				"rank": "production",
				"name": "salt",
				"value": "pepper",
			}))
			.await
			.assert_status_ok();

		let response = private
			.post("/api/inventory_secrets/for_group")
			.json(&json!({ "server_group_id": group }))
			.await;
		response.assert_status_ok();
		let body = response.text();
		assert!(
			!body.contains("pepper"),
			"a value must not be listed: {body}"
		);

		let listed: Value = serde_json::from_str(&body).expect("json");
		let listed = listed.as_array().expect("array");
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0]["name"], "salt");
		assert_eq!(listed[0]["rank"], "production");
		assert_eq!(listed[0]["set_by"], "admin@localhost");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_forgets_the_value() {
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
		let scope = json!({ "server_group_id": group, "rank": "production" });

		private
			.post("/api/inventory_secrets/set")
			.json(&json!({
				"server_group_id": group,
				"rank": "production",
				"name": "salt",
				"value": "pepper",
			}))
			.await
			.assert_status_ok();
		private
			.post("/api/inventory_secrets/remove")
			.json(&json!({ "server_group_id": group, "rank": "production", "name": "salt" }))
			.await
			.assert_status_ok();

		// Gone from the inventory, and gone rather than left valueless: a
		// leftover declaration would refuse every read.
		let response = private.post("/api/inventory/for_group").json(&scope).await;
		response.assert_status_ok();
		let body: Value = response.json();
		assert_eq!(body["secret_vars"], json!([]));
		assert!(body["vars"].get("salt").is_none());

		private
			.post("/api/inventory_secrets/remove")
			.json(&json!({ "server_group_id": group, "rank": "production", "name": "salt" }))
			.await
			.assert_status_not_found();
	})
	.await;
}
