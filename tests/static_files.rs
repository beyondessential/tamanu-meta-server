use axum::http::StatusCode;

#[path = "common/server.rs"]
mod test_server;

#[tokio::test(flavor = "multi_thread")]
async fn public_static_files_404() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/static/nonexistent.css").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_static_files_404() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/static/nonexistent.js").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_static_directory_listing_disabled() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/static/").await;
		// Static file serving should not allow directory listing
		assert_ne!(response.status_code(), 200);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_static_directory_listing_disabled() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/static/").await;
		// Static file serving should not allow directory listing
		assert_ne!(response.status_code(), 200);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_nonexistent_route() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/nonexistent-endpoint").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_nonexistent_route() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/nonexistent-endpoint").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_method_not_allowed() {
	test_server::run(async |_conn, public, _| {
		// Try to POST to a GET-only endpoint
		let response = public.post("/").await;
		response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_versions_method_not_allowed() {
	test_server::run(async |_conn, public, _| {
		// Try to PUT to versions list endpoint
		let response = public.put("/versions").await;
		response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_password_method_not_allowed() {
	test_server::run(async |_conn, public, _| {
		// Try to POST to password page (GET only)
		let response = public.post("/password").await;
		response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_servers_nested_routes() {
	test_server::run(async |_conn, public, _| {
		// Test that servers endpoint exists (returns empty list)
		let response = public.get("/servers").await;
		response.assert_status_ok();

		// Test nested routes under servers don't exist by default
		let response = public.get("/servers/nonexistent").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_status_endpoint_missing_server_id() {
	test_server::run(async |_conn, public, _| {
		// Try to access status endpoint without server ID
		let response = public.get("/status").await;
		response.assert_status(StatusCode::NOT_FOUND);

		let response = public.post("/status").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn public_artifacts_missing_parameters() {
	test_server::run(async |_conn, public, _| {
		// Try to access artifacts endpoint without required path parameters
		let response = public.get("/artifacts").await;
		response.assert_status(StatusCode::NOT_FOUND);

		let response = public.get("/artifacts/1.0.0").await;
		response.assert_status(StatusCode::NOT_FOUND);

		let response = public.get("/artifacts/1.0.0/mobile").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}
