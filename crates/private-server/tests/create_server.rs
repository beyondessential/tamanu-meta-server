//! Server creation via the admin API, focused on the optional URL behaviour.

use database::servers::Server;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn create_server_without_url_succeeds() {
	commons_tests::server::run(async |mut conn, _, private| {
		// No `host` at all — a device-only server.
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "kind": "central" }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();
		let server = Server::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert!(server.host.is_none(), "created without a URL");

		// Explicit whitespace-only string clears the URL too.
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "kind": "facility", "host": "  " }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();
		let server = Server::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert!(server.host.is_none(), "empty URL stored as null");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn create_server_defaults_schemeless_url_to_https() {
	commons_tests::server::run(async |mut conn, _, private| {
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "kind": "central", "host": "foo.example.com" }))
			.await;
		response.assert_status_ok();
		let id: String = response.json();
		let server = Server::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert_eq!(
			server.host.unwrap().0.to_string(),
			"https://foo.example.com/"
		);
	})
	.await
}
