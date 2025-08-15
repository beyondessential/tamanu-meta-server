#[path = "common/server.rs"]
mod test_server;

#[tokio::test(flavor = "multi_thread")]
async fn public_livez() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/livez").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_healthz() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/healthz").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_livez_post() {
	test_server::run(async |_conn, public, _| {
		let response = public.post("/livez").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_healthz_post() {
	test_server::run(async |_conn, public, _| {
		let response = public.post("/healthz").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_livez() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/livez").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_healthz() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/healthz").await;
		response.assert_status_ok();
	})
	.await
}
