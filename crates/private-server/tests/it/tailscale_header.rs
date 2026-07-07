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
