use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn dev_non_admin_header_downgrades_the_bypass() {
	commons_tests::server::run(async |_, _, private| {
		// The debug auth bypass reports the caller as an admin by default.
		let response = private
			.post("/api/commons/is_current_user_admin")
			.json(&json!({}))
			.await;
		response.assert_status_ok();
		assert!(response.json::<bool>());

		// The dev non-admin header downgrades that same bypass, so the
		// caller is reported as a non-admin.
		let response = private
			.post("/api/commons/is_current_user_admin")
			.add_header("x-canopy-dev-non-admin", "1")
			.json(&json!({}))
			.await;
		response.assert_status_ok();
		assert!(!response.json::<bool>());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn dev_non_admin_header_forbids_admin_endpoints() {
	commons_tests::server::run(async |_, _, private| {
		// An admin-gated endpoint rejects a caller carrying the non-admin
		// header, exactly as it would a real non-admin.
		let response = private
			.post("/api/healthchecks/set_source_reachability")
			.add_header("x-canopy-dev-non-admin", "1")
			.json(&json!({ "source": "alertd", "reachability": "quiet" }))
			.await;
		response.assert_status_forbidden();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "/$/status route is gone TODO we should still test this somehow"]
async fn tailscale_header_extraction() {
	commons_tests::server::run(async |_, _, private| {
		// Test without Tailscale-User-Name header
		let response = private.get("/$/status").await;
		response.assert_status_ok();
		let body = response.text();
		assert!(body.contains("Kia Ora!"));

		// Test with Tailscale-User-Name header
		let response = private
			.get("/$/status")
			.add_header("Tailscale-User-Name", "John")
			.await;
		response.assert_status_ok();
		let body = response.text();
		assert!(body.contains("Hi John!"));

		// Test with encoded user name
		let response = private
			.get("/$/status")
			.add_header("Tailscale-User-Name", "=?utf-8?q?F=C3=A9lix_S?=")
			.await;
		response.assert_status_ok();
		let body = response.text();
		assert!(body.contains("Hi Félix S!"));
	})
	.await
}
