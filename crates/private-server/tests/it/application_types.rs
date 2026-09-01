//! Application types through the admin API: the catalogue the UI reads, and the
//! billing labels an application carries.
//!
//! A type is reported and never entered, so there is nothing here about setting
//! or changing one: the flows that did are gone (see APP, "Where a type comes
//! from").
//!
//! spec: APP

use commons_tests::diesel_async::{AsyncPgConnection, SimpleAsyncConnection};
use serde_json::json;
use uuid::Uuid;

/// An application of a given type, on its own machine, in a group. Seeded
/// directly: a type is reported rather than entered, so there is no operator
/// flow that creates one.
async fn insert_application(
	conn: &mut AsyncPgConnection,
	group_id: &str,
	r#type: &str,
	rank: &str,
) -> Uuid {
	let id = Uuid::new_v4();
	let ty = r#type;
	conn.batch_execute(&format!(
		"WITH m AS (INSERT INTO machines (id, group_id) VALUES ('{id}', '{group_id}') RETURNING id) \
		 INSERT INTO applications (id, host, type, rank, group_id, machine_id) \
		 VALUES ('{id}', 'https://{id}.example.com', '{ty}', '{rank}', '{group_id}', '{id}')"
	))
	.await
	.expect("insert application");
	id
}

/// The type catalogue is what the UI reads to decide what to present, so it has
/// to describe every type's capabilities. It offers no roles to choose from: a
/// type is reported, never entered.
#[tokio::test(flavor = "multi_thread")]
async fn types_endpoint_describes_every_type() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.post("/api/commons/products").json(&json!({})).await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		let types = body.as_array().expect("array of types");
		assert_eq!(types.len(), 4, "the set is closed: {types:?}");

		let find = |name: &str| {
			types
				.iter()
				.find(|t| t["type"] == name)
				.unwrap_or_else(|| panic!("{name} missing from the catalogue"))
				.clone()
		};

		// The role is in the type, so a central and a facility are two entries
		// rather than one with a list of roles hanging off it.
		let central = find("tamanu-central");
		assert_eq!(central["caps"]["version_tracking"], "tracked");
		assert_eq!(central["caps"]["public_listing"], true);
		assert_eq!(central["label"], "Tamanu central");

		let facility = find("tamanu-facility");
		assert_eq!(facility["caps"]["version_tracking"], "tracked");
		assert_eq!(
			facility["caps"]["public_listing"], false,
			"only a central is publicly listable"
		);

		// A Canopy instance reports its own build version, which Canopy holds no
		// release train for: presented, but graded against nothing.
		let canopy = find("canopy");
		assert_eq!(canopy["caps"]["version_tracking"], "reported");

		let senaite = find("senaite");
		assert_eq!(senaite["caps"]["version_tracking"], "absent");
	})
	.await
}

/// The server detail view renders the server's *own* billing labels, so the page
/// agrees with what canopy hands that server's device. Rendering its group's
/// would show a SENAITE server in a Tamanu group an attribution it never gets.
#[tokio::test(flavor = "multi_thread")]
async fn detail_billing_labels_are_the_servers_own() {
	commons_tests::server::run(async |mut conn, _, private| {
		let group = private
			.post("/api/server_groups/create")
			.json(&json!({ "name": "Pacific" }))
			.await;
		group.assert_status_ok();
		let group_body: serde_json::Value = group.json();
		let group_id = group_body["id"].as_str().unwrap().to_string();

		// A production Tamanu member, so the group's highest rank is production
		// and its members span types.
		insert_application(&mut conn, &group_id, "tamanu-central", "production").await;
		let lims_id = insert_application(&mut conn, &group_id, "senaite", "clone")
			.await
			.to_string();

		let response = private
			.post("/api/servers/get_detail")
			.json(&json!({ "server_id": lims_id }))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		let labels: std::collections::HashMap<String, String> = body["billing_labels"]
			.as_array()
			.expect("billing labels")
			.iter()
			.map(|t| {
				(
					t["key"].as_str().unwrap().to_string(),
					t["value"].as_str().unwrap().to_string(),
				)
			})
			.collect();

		assert_eq!(
			labels.get("billing.product").map(String::as_str),
			Some("senaite")
		);
		// Its own rank, not the group's highest.
		assert_eq!(
			labels.get("billing.stage").map(String::as_str),
			Some("clone")
		);
		// The deployment still comes from the group.
		assert_eq!(
			labels.get("billing.deployment").map(String::as_str),
			Some("pacific")
		);
	})
	.await
}
