//! The application endpoints live under `/api/applications`, and the prefix
//! they used to live under still lands.
//!
//! `servers` named the box and the workload at once. Each has its own word
//! now, so the endpoints about a workload moved — and a link or a script
//! written against the old prefix outlives the rename.

use axum::http::StatusCode;

#[tokio::test(flavor = "multi_thread")]
async fn the_application_endpoints_answer_under_applications() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/applications/list")
			.json(&serde_json::json!({}))
			.await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_old_prefix_redirects_permanently() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/servers/list")
			.json(&serde_json::json!({}))
			.await;
		// Permanent, not temporary: the caller has to repeat the POST and its
		// body rather than retrying as a GET, which is what a 302 would turn
		// this into.
		response.assert_status(StatusCode::PERMANENT_REDIRECT);
		response.assert_header("location", "/api/applications/list");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_fleet_index_is_not_redirected_out_from_under_the_spa() {
	commons_tests::server::run(async |_conn, _, private| {
		// `/servers` without the `/api` prefix is the SPA's own fleet route,
		// and shares only a word with the endpoints that moved.
		let response = private.get("/servers/figures").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}
