use axum::{body::Bytes, http::StatusCode};

#[path = "common/server.rs"]
mod test_server;

#[tokio::test(flavor = "multi_thread")]
async fn timesync_endpoint_rejects_get() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/timesync").await;
		response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn timesync_endpoint_accepts_post() {
	test_server::run(async |_conn, public, _| {
		// Create a minimal valid timesimp request
		let request_bytes = vec![0, 0, 0, 0, 0, 0, 0, 0]; // Basic 8-byte request

		let response = public
			.post("/timesync")
			.bytes(Bytes::from(request_bytes))
			.await;

		// Should accept the request (might return error due to invalid format, but not 405)
		assert_ne!(response.status_code(), 405);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn timesync_endpoint_with_empty_body() {
	test_server::run(async |_conn, public, _| {
		let response = public.post("/timesync").bytes(Bytes::new()).await;

		// Should handle empty body gracefully
		assert_ne!(response.status_code(), 405);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn timesync_endpoint_returns_bytes() {
	test_server::run(async |_conn, public, _| {
		// Create a minimal request that might be parseable
		let request_bytes = vec![0; 32]; // 32-byte request

		let response = public
			.post("/timesync")
			.bytes(Bytes::from(request_bytes))
			.await;

		// Response should be binary data, not JSON or HTML
		let content_type = response.headers().get("content-type");
		if let Some(ct) = content_type {
			assert_ne!(ct, "application/json");
			assert_ne!(ct, "text/html; charset=utf-8");
		}
	})
	.await
}
