//! Minting, listing, and revoking the calendar feeds operators subscribe to.

use database::calendar_tokens::TOKEN_PREFIX;

#[tokio::test(flavor = "multi_thread")]
async fn mint_hands_out_a_url_once_and_revoke_ends_it() {
	commons_tests::server::run(async |_conn, _public, private| {
		let minted = private
			.post("/api/calendar_tokens/mint")
			.json(&serde_json::json!({ "name": "  ops team  " }))
			.await;
		assert_eq!(minted.status_code().as_u16(), 200);
		let minted: serde_json::Value = minted.json();

		let path = minted["path"].as_str().expect("path");
		assert!(path.contains(TOKEN_PREFIX), "{path}");
		assert!(path.ends_with("/upgrades.ics"), "{path}");
		assert_eq!(minted["token"]["name"], "ops team", "the name is trimmed");

		let listed: serde_json::Value = private
			.post("/api/calendar_tokens/list")
			.json(&serde_json::json!({}))
			.await
			.json();
		let listed = listed.as_array().expect("array");
		assert_eq!(listed.len(), 1);
		assert!(
			listed[0].get("url").is_none() && listed[0].get("path").is_none(),
			"the URL is shown once at minting and never again: {listed:?}"
		);

		let id = minted["token"]["id"].clone();
		let revoked = private
			.post("/api/calendar_tokens/revoke")
			.json(&serde_json::json!({ "id": id }))
			.await;
		assert_eq!(revoked.status_code().as_u16(), 200);

		let listed: serde_json::Value = private
			.post("/api/calendar_tokens/list")
			.json(&serde_json::json!({}))
			.await
			.json();
		assert!(listed[0]["revoked_at"].is_string());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_feed_needs_a_name() {
	commons_tests::server::run(async |_conn, _public, private| {
		let resp = private
			.post("/api/calendar_tokens/mint")
			.json(&serde_json::json!({ "name": "   " }))
			.await;
		assert_eq!(resp.status_code().as_u16(), 400);
	})
	.await
}
