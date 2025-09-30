#[tokio::test(flavor = "multi_thread")]
async fn livez() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.get("/$/livez").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn healthz() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.get("/$/healthz").await;
		response.assert_status_ok();
	})
	.await
}
