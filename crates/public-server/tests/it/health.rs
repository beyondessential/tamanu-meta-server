#[tokio::test(flavor = "multi_thread")]
async fn livez() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/livez").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn healthz() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/healthz").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn livez_post() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.post("/livez").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn healthz_post() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.post("/healthz").await;
		response.assert_status_ok();
	})
	.await
}
