use commons_tests::server;
use diesel_async::SimpleAsyncConnection;
use http::StatusCode;

#[tokio::test(flavor = "multi_thread")]
async fn server_versions_wrong_secret() {
	server::run(|_conn, public, _private| async move {
		// Request with wrong secret should fail
		let response = public.get("/server-versions?s=wrong-secret").await;
		assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn server_versions_missing_query_parameter() {
	server::run(|_conn, public, _private| async move {
		// Request without query parameter should fail with bad request
		let response = public.get("/server-versions").await;
		assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn server_versions_correct_secret() {
	server::run(|mut conn, public, _private| async move {
		// Set up test data with multiple servers
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Production Server 1', 'https://prod1.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Production Server 2', 'https://prod2.example.com', 'production', 'central'),
			('33333333-3333-3333-3333-333333333333', 'Clone Server', 'https://clone.example.com', 'clone', 'central'),
			('44444444-4444-4444-4444-444444444444', 'Facility Server', 'https://facility.example.com', 'production', 'facility')",
		)
		.await
		.unwrap();

		// Add versions and statuses
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, published, changelog) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 2, 10, 0, true, 'Version 2.10.0'),
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 2, 11, 0, true, 'Version 2.11.0'),
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 2, 12, 0, true, 'Version 2.12.0')",
		)
		.await
		.unwrap();

		// Add statuses for servers
		conn.batch_execute(
			"INSERT INTO statuses (id, server_id, version, extra) VALUES
			('11111111-1111-1111-1111-111111111111', '11111111-1111-1111-1111-111111111111', '2.12.0', '{}'),
			('22222222-2222-2222-2222-222222222222', '22222222-2222-2222-2222-222222222222', '2.10.0', '{}')",
		)
		.await
		.unwrap();

		// Request with correct secret should succeed
		let response = public.get("/server-versions?s=test-secret").await;
		assert_eq!(response.status_code(), StatusCode::OK);
		let body = response.text();
		assert!(body.contains("Production Server Versions"));

		// Should display production central servers only
		assert!(body.contains("Production Server 1"));
		assert!(body.contains("Production Server 2"));
		assert!(body.contains("2.12.0"));
		assert!(body.contains("2.10.0"));

		// Should NOT display clone or facility servers
		assert!(!body.contains("Clone Server"));
		assert!(!body.contains("Facility Server"));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn server_versions_constant_time_comparison() {
	server::run(|_conn, public, _private| async move {
		// Different length should fail
		let response = public.get("/server-versions?s=short").await;
		assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);

		// Same length but wrong secret should fail
		let response = public.get("/server-versions?s=wrong-secret").await;
		assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);

		// Correct secret should succeed
		let response = public.get("/server-versions?s=test-secret").await;
		assert_eq!(response.status_code(), StatusCode::OK);
	})
	.await;
}
