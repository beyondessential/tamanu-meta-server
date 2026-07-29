//! Endpoint tests for the operator-facing `/api/domains/*` fns — the zone list
//! read from Canopy's configuration, and claiming/releasing a group's domains
//! against it.

use commons_tests::diesel_async::{AsyncPgConnection, SimpleAsyncConnection};
use uuid::Uuid;

async fn insert_group(conn: &mut AsyncPgConnection, name: &str) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{id}', '{name}')"
	))
	.await
	.expect("insert group");
	id
}

/// Each nextest test runs in its own process, so setting the zone configuration
/// here is isolated to this test. It has to happen before the server is built:
/// the zones are read once, at startup.
fn configure_zones(spec: &str) {
	unsafe { std::env::set_var("CANOPY_DNS_ZONES", spec) };
}

#[tokio::test(flavor = "multi_thread")]
async fn zones_lists_the_configured_zones() {
	configure_zones("tamanu.app=Z1ABC, senaite.app=Z2DEF");

	commons_tests::server::run(async move |_conn, _public, private| {
		let resp = private
			.post("/api/domains/zones")
			.json(&serde_json::json!({}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body.as_array().expect("array").len(), 2);
		assert_eq!(body[0]["apex"], "tamanu.app");
		assert_eq!(body[0]["provider_zone_id"], "Z1ABC");
		assert_eq!(body[1]["apex"], "senaite.app");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn claim_release_roundtrip() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;

		// Nothing claimed to start with.
		let resp = private
			.post("/api/domains/for_group")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		assert!(
			resp.json::<serde_json::Value>()
				.as_array()
				.unwrap()
				.is_empty()
		);

		// Claimed, normalised, and matched to its zone.
		let resp = private
			.post("/api/domains/claim")
			.json(&serde_json::json!({
				"server_group_id": group,
				"domain": "Fiji.Tamanu.App.",
			}))
			.await;
		resp.assert_status_ok();
		let claim: serde_json::Value = resp.json();
		assert_eq!(claim["domain"], "fiji.tamanu.app");
		assert_eq!(claim["zone"], "tamanu.app");
		assert_eq!(claim["group_id"], group.to_string());
		let claim_id = claim["id"].as_str().expect("id").to_string();

		// Listed against the group.
		let resp = private
			.post("/api/domains/for_group")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		let listed: serde_json::Value = resp.json();
		assert_eq!(listed.as_array().unwrap().len(), 1);
		assert_eq!(listed[0]["domain"], "fiji.tamanu.app");

		// Released, and gone.
		let resp = private
			.post("/api/domains/release")
			.json(&serde_json::json!({"id": claim_id}))
			.await;
		resp.assert_status_ok();

		let resp = private
			.post("/api/domains/for_group")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		assert!(
			resp.json::<serde_json::Value>()
				.as_array()
				.unwrap()
				.is_empty()
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn claim_outside_the_zones_is_a_400() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;
		let resp = private
			.post("/api/domains/claim")
			.json(&serde_json::json!({
				"server_group_id": group,
				"domain": "fiji.example.com",
			}))
			.await;
		resp.assert_status_bad_request();

		// So is a name that isn't a domain at all.
		let resp = private
			.post("/api/domains/claim")
			.json(&serde_json::json!({"server_group_id": group, "domain": "nope"}))
			.await;
		resp.assert_status_bad_request();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn overlapping_claim_is_a_409() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let fiji = insert_group(&mut conn, "fiji").await;
		let samoa = insert_group(&mut conn, "samoa").await;

		private
			.post("/api/domains/claim")
			.json(&serde_json::json!({
				"server_group_id": fiji,
				"domain": "fiji.tamanu.app",
			}))
			.await
			.assert_status_ok();

		for domain in ["fiji.tamanu.app", "sub.fiji.tamanu.app", "tamanu.app"] {
			let resp = private
				.post("/api/domains/claim")
				.json(&serde_json::json!({
					"server_group_id": samoa,
					"domain": domain,
				}))
				.await;
			assert_eq!(
				resp.status_code(),
				axum::http::StatusCode::CONFLICT,
				"claiming {domain} should conflict"
			);
		}
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn releasing_an_unknown_claim_is_a_404() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |_conn, _public, private| {
		let resp = private
			.post("/api/domains/release")
			.json(&serde_json::json!({"id": Uuid::new_v4()}))
			.await;
		resp.assert_status_not_found();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claim_whose_zone_left_the_configuration_reads_as_unmatched() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;
		private
			.post("/api/domains/claim")
			.json(&serde_json::json!({
				"server_group_id": group,
				"domain": "fiji.tamanu.app",
			}))
			.await
			.assert_status_ok();

		// Insert a claim in a zone this instance is not configured for — the
		// state a claim lands in when its zone leaves the configuration.
		conn.batch_execute(&format!(
			"INSERT INTO server_group_domains (group_id, domain) \
			 VALUES ('{group}', 'old.senaite.app')"
		))
		.await
		.expect("insert stale claim");

		let resp = private
			.post("/api/domains/for_group")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		let listed: serde_json::Value = resp.json();
		let by_domain: std::collections::HashMap<&str, &serde_json::Value> = listed
			.as_array()
			.unwrap()
			.iter()
			.map(|row| (row["domain"].as_str().unwrap(), row))
			.collect();
		assert_eq!(by_domain["fiji.tamanu.app"]["zone"], "tamanu.app");
		assert!(
			by_domain["old.senaite.app"]["zone"].is_null(),
			"a claim with no configured zone should report no zone"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_zones_configured_nothing_can_be_claimed() {
	configure_zones("");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;

		let resp = private
			.post("/api/domains/zones")
			.json(&serde_json::json!({}))
			.await;
		resp.assert_status_ok();
		assert!(
			resp.json::<serde_json::Value>()
				.as_array()
				.unwrap()
				.is_empty()
		);

		let resp = private
			.post("/api/domains/claim")
			.json(&serde_json::json!({
				"server_group_id": group,
				"domain": "fiji.tamanu.app",
			}))
			.await;
		resp.assert_status_bad_request();
	})
	.await;
}

// ── Whether the grants are worth offering ───────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn grant_availability_reports_unconfigured_with_no_zones_and_no_claims() {
	configure_zones("");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;

		let resp = private
			.post("/api/domains/grant_availability")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(
			body["state"], "unconfigured",
			"nothing configured and nothing claimed: the feature is not in use here"
		);
		assert_eq!(body["group_domains"].as_array().expect("array").len(), 0);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_availability_reports_no_group_domain_once_zones_are_configured() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;

		let resp = private
			.post("/api/domains/grant_availability")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(
			body["state"], "no_group_domain",
			"the feature exists, but this group controls nothing to grant over"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_availability_names_the_domains_once_the_group_controls_one() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;
		private
			.post("/api/domains/claim")
			.json(&serde_json::json!({
				"server_group_id": group,
				"domain": "fiji.tamanu.app",
			}))
			.await
			.assert_status_ok();

		let resp = private
			.post("/api/domains/grant_availability")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["state"], "available");
		assert_eq!(body["group_domains"][0], "fiji.tamanu.app");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_availability_stays_in_use_for_a_claim_whose_zone_was_withdrawn() {
	// A claim outlives the zone it was made against, so "no zones" alone does not
	// mean the feature is unused — an operator still has claims to tidy up.
	configure_zones("");

	commons_tests::server::run(async move |mut conn, _public, private| {
		let group = insert_group(&mut conn, "fiji").await;
		let other = insert_group(&mut conn, "samoa").await;
		conn.batch_execute(&format!(
			"INSERT INTO server_group_domains (group_id, domain) \
			 VALUES ('{other}', 'samoa.tamanu.app')"
		))
		.await
		.expect("insert claim");

		let resp = private
			.post("/api/domains/grant_availability")
			.json(&serde_json::json!({"server_group_id": group}))
			.await;
		resp.assert_status_ok();
		assert_eq!(
			resp.json::<serde_json::Value>()["state"],
			"no_group_domain",
			"another group's claim keeps the feature in use fleet-wide"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_availability_handles_a_server_with_no_group() {
	configure_zones("tamanu.app=Z1ABC");

	commons_tests::server::run(async move |_conn, _public, private| {
		let resp = private
			.post("/api/domains/grant_availability")
			.json(&serde_json::json!({"server_group_id": null}))
			.await;
		resp.assert_status_ok();
		assert_eq!(
			resp.json::<serde_json::Value>()["state"],
			"no_group_domain",
			"a domain is controlled by a group, so an ungrouped server has none"
		);
	})
	.await;
}
