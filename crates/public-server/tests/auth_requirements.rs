use commons_tests::diesel_async::SimpleAsyncConnection;
use percent_encoding::utf8_percent_encode;

// Tests for endpoints that require authentication but don't have valid auth
// These tests verify that the authentication checks are in place

#[tokio::test(flavor = "multi_thread")]
async fn artifacts_create_requires_releaser_auth() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create a version first
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		// Try to create artifact without authentication
		let response = public
			.post("/artifacts/1.0.0/mobile/android")
			.text("https://example.com/download.apk")
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_create_requires_releaser_auth() {
	commons_tests::server::run(async |_conn, public, _| {
		// Try to create version without authentication
		let response = public
			.post("/versions/2.0.0")
			.text("# New Version\n\nChangelog content")
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_delete_requires_admin_auth() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create a version first
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		// Try to delete version without authentication
		let response = public.delete("/versions/1.0.0").await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_create_requires_server_auth() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create a server
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES ('11111111-1111-1111-1111-111111111111', 'https://test.com', 'tamanu')",
		)
		.await
		.unwrap();

		// Try to create status without authentication
		let response = public
			.post("/status/11111111-1111-1111-1111-111111111111")
			.json(&serde_json::json!({"uptime": 3600}))
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_header_missing_completely() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		// Ensure no authentication headers are present
		let response = public
			.post("/versions/1.0.1")
			.text("changelog")
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_header_invalid_certificate() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		// Send invalid certificate data
		let response = public
			.post("/versions/1.0.1")
			.add_header("mtls-certificate", "invalid-certificate-data")
			.text("changelog")
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_header_malformed_pem() {
	commons_tests::server::run(async |_conn, public, _| {
		// Send malformed PEM data
		let response = public
			.post("/artifacts/1.0.0/mobile/android")
			.add_header(
				"mtls-certificate",
				utf8_percent_encode(
					"-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----",
					&percent_encoding::NON_ALPHANUMERIC,
				)
				.to_string(),
			)
			.text("https://example.com/download.apk")
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_header_empty() {
	commons_tests::server::run(async |_conn, public, _| {
		// Send empty certificate header
		let response = public
			.post("/artifacts/1.0.0/mobile/android")
			.add_header("mtls-certificate", "")
			.text("https://example.com/download.apk")
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_with_ssl_client_cert_header() {
	commons_tests::server::run(async |_conn, public, _| {
		// Test alternative header name
		let response = public
			.post("/artifacts/1.0.0/mobile/android")
			.add_header("ssl-client-cert", "invalid-cert")
			.text("https://example.com/download.apk")
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_unauthorized_server_device_mismatch() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create server without device association
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind, device_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'https://test.com', 'tamanu', null)",
		)
		.await
		.unwrap();

		// Try to create status - should fail even if we had auth because device doesn't match
		let response = public
			.post("/status/11111111-1111-1111-1111-111111111111")
			.json(&serde_json::json!({"uptime": 3600}))
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn nonexistent_server_status_create() {
	commons_tests::server::run(async |_conn, public, _| {
		// Try to create status for non-existent server
		let response = public
			.post("/status/99999999-9999-9999-9999-999999999999")
			.json(&serde_json::json!({"uptime": 3600}))
			.await;

		response.assert_status_not_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_endpoints_no_auth_required() {
	commons_tests::server::run(async |_conn, public, _| {
		// These endpoints should work without authentication
		let endpoints = vec![
			"/",
			"/password",
			"/versions",
			"/servers",
			"/livez",
			"/healthz",
		];

		for endpoint in endpoints {
			let response = public.get(endpoint).await;
			response.assert_status_ok();
		}
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_endpoints_accessible() {
	commons_tests::server::run(async |_conn, _, private| {
		// Private endpoints should be accessible (though they might need other auth)
		let endpoints = vec!["/$/status", "/$/livez", "/$/healthz"];

		for endpoint in endpoints {
			let response = private.get(endpoint).await;
			response.assert_status_ok();
		}
	})
	.await
}
