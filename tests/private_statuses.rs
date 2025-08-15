use axum::http::StatusCode;
use diesel_async::SimpleAsyncConnection;

#[path = "common/server.rs"]
mod test_server;

#[tokio::test(flavor = "multi_thread")]
async fn private_status_page() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/status").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_page_with_servers() {
	test_server::run(async |mut conn, _, private| {
		// Insert test data for status page
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Test Server 1', 'https://test1.example.com', 'production', 'facility'),
			('22222222-2222-2222-2222-222222222222', 'Test Server 2', 'https://test2.example.com', 'test', 'facility')",
		)
		.await
		.unwrap();

		let response = private.get("/$/status").await;
		eprintln!("{}", response.text());
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");

		let body = response.text();
		assert!(body.contains("Test Server 1") || body.contains("Test Server 2"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_page_with_versions() {
	test_server::run(async |mut conn, _, private| {
		// Insert test versions and servers with statuses
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, published) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 1, 0, 0, 'Version 1.0.0', true),
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 1, 0, 1, 'Version 1.0.1', true);

			INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Prod Server', 'https://prod.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, extra) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.1', '{}'::jsonb)",
		)
		.await
		.unwrap();

		let response = private.get("/$/status").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");

		let body = response.text();
		assert!(body.contains("Prod Server"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_reload_endpoint_post() {
	test_server::run(async |_conn, _, private| {
		let response = private.post("/$/reload").await;
		response.assert_status_ok();
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_reload_endpoint_get_not_allowed() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/reload").await;
		response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_empty_database() {
	test_server::run(async |_conn, _, private| {
		// Test status page with no data
		let response = private.get("/$/status").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");

		let body = response.text();
		// Should still render the page template even with no data
		assert!(body.contains("<html") || body.contains("<!DOCTYPE"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_with_mixed_server_ranks() {
	test_server::run(async |mut conn, _, private| {
		// Insert servers with different ranks
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Production Server', 'https://prod.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Staging Server', 'https://staging.example.com', 'test', 'central'),
			('33333333-3333-3333-3333-333333333333', 'Dev Server', 'https://dev.example.com', 'demo', 'central')",
		)
		.await
		.unwrap();

		let response = private.get("/$/status").await;
		response.assert_status_ok();

		let body = response.text();
		assert!(body.contains("Production Server"));
		assert!(body.contains("Staging Server"));
		assert!(body.contains("Dev Server"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_with_server_statuses() {
	test_server::run(async |mut conn, _, private| {
		// Insert servers and their status reports
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Test Server', 'https://test.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, latency_ms, error, extra) VALUES
			('11111111-1111-1111-1111-111111111111', '1.2.3', 150, null, '{\"uptime\": 3600}'::jsonb),
			('11111111-1111-1111-1111-111111111111', '1.2.4', 200, 'Connection timeout', '{}'::jsonb)",
		)
		.await
		.unwrap();

		let response = private.get("/$/status").await;
		response.assert_status_ok();

		let body = response.text();
		assert!(body.contains("Test Server"));
	})
	.await
}
