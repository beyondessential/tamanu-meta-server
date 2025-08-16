use axum::http::StatusCode;
use diesel_async::SimpleAsyncConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[path = "common/server.rs"]
mod test_server;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct ServerData {
	server: ServerInfo,
	device: Option<Value>,
	status: Option<StatusInfo>,
	up: String,
	since: Option<i64>,
	platform: Option<String>,
	postgres: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct ServerInfo {
	id: String,
	name: Option<String>,
	host: String,
	kind: String,
	rank: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct StatusInfo {
	id: String,
	created_at: String,
	server_id: String,
	device_id: Option<String>,
	latency_ms: Option<i32>,
	version: Option<String>,
	error: Option<String>,
	remote_ip: Option<String>,
	extra: Value,
}

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
		eprintln!("{}", response.text());
		response.assert_status_ok();

		let body = response.text();
		assert!(body.contains("Test Server"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_empty_database() {
	test_server::run(async |_conn, _, private| {
		let response = private.get("/$/status.json").await;
		response.assert_status_ok();
		response.assert_header("content-type", "application/json");

		let servers: Vec<ServerData> = response.json();
		assert!(servers.is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_basic_server() {
	test_server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Test Server', 'https://test.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();
		response.assert_header("content-type", "application/json");

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 1);

		let server = &servers[0];
		assert_eq!(server.server.name, Some("Test Server".to_string()));
		assert_eq!(server.server.host, "https://test.example.com");
		assert_eq!(server.server.rank, Some("production".to_string()));
		assert_eq!(server.server.kind, "central");
		assert_eq!(server.up, "gone"); // No status means "gone"
		assert!(server.status.is_none());
		assert!(server.since.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_server_with_recent_status() {
	test_server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Active Server', 'https://active.example.com', 'production', 'facility');

			INSERT INTO statuses (server_id, version, latency_ms, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.2.3', 100, '{\"uptime\": 3600}'::jsonb, NOW())"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 1);

		let server = &servers[0];
		assert_eq!(server.server.name, Some("Active Server".to_string()));
		assert_eq!(server.up, "up"); // Recent status means "up"
		assert!(server.status.is_some());
		assert!(server.since.is_some());
		assert!(server.since.unwrap() < 2); // Should be less than 2 minutes ago

		let status = server.status.as_ref().unwrap();
		assert_eq!(status.version, Some("1.2.3".to_string()));
		assert_eq!(status.latency_ms, Some(100));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_server_status_ages() {
	test_server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Down Server', 'https://down.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Away Server', 'https://away.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', NOW() - INTERVAL '45 minutes'),
			('22222222-2222-2222-2222-222222222222', '1.0.0', NOW() - INTERVAL '15 minutes')"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 2);

		// Find servers by name since ordering might vary
		let down_server = servers.iter().find(|s| s.server.name.as_deref() == Some("Down Server")).unwrap();
		let away_server = servers.iter().find(|s| s.server.name.as_deref() == Some("Away Server")).unwrap();

		assert_eq!(down_server.up, "down"); // 45 minutes ago
		assert_eq!(away_server.up, "away"); // 15 minutes ago
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_platform_detection() {
	test_server::run(async |mut conn, _, private| {
		// Insert servers with different PostgreSQL versions to test platform detection
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Windows Server', 'https://win.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Linux Server', 'https://linux.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', '{\"pgVersion\": \"PostgreSQL 13.7 on x86_64-pc-windows-msvc, compiled by Visual C++ build 1914\"}'::jsonb, NOW()),
			('22222222-2222-2222-2222-222222222222', '1.0.0', '{\"pgVersion\": \"PostgreSQL 17.2, (x86_64-pc-linux-gnu, compiled by gcc)\"}'::jsonb, NOW())"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 2);

		let win_server = servers.iter().find(|s| s.server.name.as_deref() == Some("Windows Server")).unwrap();
		let linux_server = servers.iter().find(|s| s.server.name.as_deref() == Some("Linux Server")).unwrap();

		assert_eq!(win_server.platform, Some("Windows".to_string()));
		assert_eq!(linux_server.platform, Some("Linux".to_string()));
		assert_eq!(win_server.postgres, Some("13.7".to_string()));
		assert_eq!(linux_server.postgres, Some("17.2".to_string()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_server_with_error() {
	test_server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Error Server', 'https://error.example.com', 'test', 'facility');

			INSERT INTO statuses (server_id, version, latency_ms, error, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', 5000, 'Connection timeout', NOW())"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 1);

		let server = &servers[0];
		assert_eq!(server.server.name, Some("Error Server".to_string()));
		assert_eq!(server.up, "up"); // Still "up" because it's recent

		let status = server.status.as_ref().unwrap();
		assert_eq!(status.error, Some("Connection timeout".to_string()));
		assert_eq!(status.latency_ms, Some(5000));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_mixed_server_ranks() {
	test_server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Production Server', 'https://prod.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Test Server', 'https://test.example.com', 'test', 'central'),
			('33333333-3333-3333-3333-333333333333', 'Demo Server', 'https://demo.example.com', 'demo', 'central')"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 3);

		assert_eq!(servers[0].server.name, Some("Production Server".to_string()));
		assert_eq!(servers[0].server.rank, Some("production".to_string()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn private_status_json_unnamed_servers_excluded() {
	test_server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Named Server', 'https://named.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', NULL, 'https://unnamed.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 1);
		assert_eq!(servers[0].server.name, Some("Named Server".to_string()));
	})
	.await
}
