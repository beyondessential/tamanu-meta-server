//! Product classification through the admin API: how the product/kind pair is
//! settled on create and update, and how the version is presented for a
//! product that has none.
//!
//! spec: APP

use commons_types::server::{kind::ServerKind, product::Product};
use database::applications::Application;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn create_defaults_to_tamanu_and_round_trips_a_product() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Omitting the product classifies the server as Tamanu, so an existing
		// caller that never heard of the field keeps working.
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "kind": "central" }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();
		let server = Application::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert_eq!(server.product, Product::Tamanu);

		let response = private
			.post("/api/servers/create")
			.json(&json!({ "product": "senaite", "kind": "standalone" }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();
		let server = Application::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert_eq!(server.product, Product::Senaite);
		assert_eq!(server.kind, ServerKind::Standalone);
	})
	.await
}

/// A role its product doesn't define would leave the server misclassified from
/// the moment it exists, so creation refuses it outright.
#[tokio::test(flavor = "multi_thread")]
async fn create_rejects_a_role_the_product_does_not_define() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "product": "senaite", "kind": "central" }))
			.await;
		response.assert_status_bad_request();
	})
	.await
}

/// Reclassifying a Tamanu central as SENAITE, which has no central role,
/// carries the server to the new product's default role rather than stranding
/// it on one its product doesn't have.
#[tokio::test(flavor = "multi_thread")]
async fn changing_product_moves_an_orphaned_kind_to_the_new_default() {
	commons_tests::server::run(async |mut conn, _, private| {
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "kind": "central" }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": id,
				"data": { "product": "senaite" },
			}))
			.await;
		response.assert_status_ok();

		let server = Application::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert_eq!(server.product, Product::Senaite);
		assert_eq!(
			server.kind,
			ServerKind::Standalone,
			"the kind follows the product rather than staying central"
		);
	})
	.await
}

/// A product change that keeps a role the new product also defines leaves the
/// role alone.
#[tokio::test(flavor = "multi_thread")]
async fn changing_product_keeps_a_kind_the_new_product_defines() {
	commons_tests::server::run(async |mut conn, _, private| {
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "product": "senaite", "kind": "standalone" }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();

		// Canopy also defines standalone, so nothing has to move.
		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": id,
				"data": { "product": "canopy" },
			}))
			.await;
		response.assert_status_ok();

		let server = Application::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert_eq!(server.product, Product::Canopy);
		assert_eq!(server.kind, ServerKind::Standalone);
	})
	.await
}

/// An explicitly requested role the target product doesn't define is a bad
/// request rather than something quietly corrected.
#[tokio::test(flavor = "multi_thread")]
async fn update_rejects_a_role_the_product_does_not_define() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "product": "senaite", "kind": "standalone" }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": id,
				"data": { "kind": "facility" },
			}))
			.await;
		response.assert_status_bad_request();
	})
	.await
}

/// The product catalogue is what the UI reads to decide what to present, so it
/// has to describe every product's capabilities and roles.
#[tokio::test(flavor = "multi_thread")]
async fn products_endpoint_describes_every_product() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.post("/api/commons/products").json(&json!({})).await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		let products = body.as_array().expect("array of products");

		let find = |name: &str| {
			products
				.iter()
				.find(|p| p["product"] == name)
				.unwrap_or_else(|| panic!("{name} missing from the catalogue"))
				.clone()
		};

		let tamanu = find("tamanu");
		assert_eq!(tamanu["caps"]["version_tracking"], "tracked");
		assert_eq!(tamanu["caps"]["public_listing"], true);
		assert_eq!(tamanu["kinds"], json!(["central", "facility"]));
		assert_eq!(tamanu["default_kind"], "central");

		// A canopy instance reports its own build version, which canopy holds no
		// release train for: presented, but graded against nothing.
		let canopy = find("canopy");
		assert_eq!(canopy["caps"]["version_tracking"], "reported");
		assert_eq!(canopy["caps"]["public_listing"], false);

		let senaite = find("senaite");
		assert_eq!(senaite["caps"]["version_tracking"], "absent");
		assert_eq!(senaite["kinds"], json!(["standalone"]));
	})
	.await
}

/// A public name already set survives the server becoming ineligible to be
/// listed: reclassification shouldn't destroy operator data, and the name takes
/// effect again if the server becomes eligible once more.
#[tokio::test(flavor = "multi_thread")]
async fn public_name_survives_losing_eligibility() {
	commons_tests::server::run(async |mut conn, _, private| {
		let response = private
			.post("/api/servers/create")
			.json(&json!({
				"kind": "central",
				"public_name": "Island Central",
			}))
			.await;
		response.assert_status_ok();
		let id: String = response.json();
		let server_id: uuid::Uuid = id.parse().unwrap();

		// Reclassifying to SENAITE makes it ineligible: no public listing, and
		// the role moves to standalone.
		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": id,
				"data": { "product": "senaite" },
			}))
			.await;
		response.assert_status_ok();

		let server = Application::get_by_id(&mut conn, server_id).await.unwrap();
		assert_eq!(server.product, Product::Senaite);
		assert_eq!(
			server.public_name.as_deref(),
			Some("Island Central"),
			"the stored name is kept rather than cleared"
		);
		// It is nonetheless not listed while ineligible.
		let listed = Application::search_central(&mut conn, "Island", 50)
			.await
			.unwrap();
		assert!(listed.is_empty(), "ineligible applications are not offered");

		// Back to Tamanu central, and the name takes effect again.
		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": id,
				"data": { "product": "tamanu", "kind": "central", "name": "Island Central" },
			}))
			.await;
		response.assert_status_ok();

		let listed = Application::search_central(&mut conn, "Island", 50)
			.await
			.unwrap();
		assert_eq!(
			listed
				.into_iter()
				.filter_map(|s| s.public_name)
				.collect::<Vec<_>>(),
			vec!["Island Central".to_string()],
		);
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
		// and its members span products.
		let response = private
			.post("/api/servers/create")
			.json(&json!({
				"kind": "central",
				"rank": "production",
				"group_id": group_id,
			}))
			.await;
		response.assert_status_ok();

		let response = private
			.post("/api/servers/create")
			.json(&json!({
				"product": "senaite",
				"kind": "standalone",
				"rank": "clone",
				"group_id": group_id,
			}))
			.await;
		response.assert_status_ok();
		let lims_id: String = response.json();

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
		let _ = &mut conn;
	})
	.await
}
