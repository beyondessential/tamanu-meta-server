#[tokio::test(flavor = "multi_thread")]
async fn password_page() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/password").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}
