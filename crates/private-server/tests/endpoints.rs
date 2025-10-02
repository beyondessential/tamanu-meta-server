#[tokio::test(flavor = "multi_thread")]
async fn private_endpoints_accessible() {
	commons_tests::server::run(async |_conn, _, private| {
		// Private endpoints should be accessible (though they might need other auth)
		let endpoints = vec!["/", "/livez", "/healthz"];

		for endpoint in endpoints {
			let response = private.get(endpoint).await;
			response.assert_status_ok();
		}
	})
	.await
}
