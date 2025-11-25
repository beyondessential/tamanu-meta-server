use axum::http::StatusCode;
use commons_tests::diesel_async::SimpleAsyncConnection;

#[tokio::test(flavor = "multi_thread")]
async fn index_page() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn index_page_with_versions() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES
			(1, 0, 0, '# Initial Release\n\nFirst version of the software.', 'published'),
			(1, 0, 1, '# Bug Fixes\n\n- Fixed critical bug\n- Improved performance', 'published')",
		)
		.await
		.unwrap();

		let response = public.get("/").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");

		// The response should contain rendered HTML with version information
		let body = response.text();
		assert!(body.contains("1.0") || body.contains("1.0.0") || body.contains("1.0.1"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn error_redirect() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/errors/some-error-slug").await;
		response.assert_status(StatusCode::TEMPORARY_REDIRECT);

		let location = response.headers().get("location").unwrap();
		let location_str = location.to_str().unwrap();
		assert!(location_str.contains("#some-error-slug"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn error_redirect_with_special_characters() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public
			.get("/errors/error-with-dashes_and_underscores")
			.await;
		response.assert_status(StatusCode::TEMPORARY_REDIRECT);

		let location = response.headers().get("location").unwrap();
		let location_str = location.to_str().unwrap();
		assert!(location_str.contains("#error-with-dashes_and_underscores"));
	})
	.await
}
