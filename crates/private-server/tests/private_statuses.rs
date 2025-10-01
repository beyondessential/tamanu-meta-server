use commons_tests::diesel_async::SimpleAsyncConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct ServerData {
	server: ServerInfo,
	device: Option<Value>,
	status: Option<StatusInfo>,
	up: String,
	since: Option<String>,
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

	version: Option<String>,
	extra: Value,
}

#[tokio::test(flavor = "multi_thread")]
async fn status_page() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.get("/$/status").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_empty_database() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.get("/$/status.json").await;
		response.assert_status_ok();
		response.assert_header("content-type", "application/json");

		let servers: Vec<ServerData> = response.json();
		assert!(servers.is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_basic_server() {
	commons_tests::server::run(async |mut conn, _, private| {
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
async fn status_json_server_with_recent_status() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Active Server', 'https://active.example.com', 'production', 'facility');

			INSERT INTO statuses (server_id, version, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.2.3', '{\"uptime\": 3600}'::jsonb, NOW())"
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
		let since_text = server.since.as_ref().unwrap();
		assert!(since_text.contains("ms"));

		let status = server.status.as_ref().unwrap();
		assert_eq!(status.version, Some("1.2.3".to_string()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_server_status_ages() {
	commons_tests::server::run(async |mut conn, _, private| {
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
async fn status_json_platform_detection() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Insert servers with different PostgreSQL versions to test platform detection
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Windows Server', 'https://win.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Linux Server', 'https://linux.example.com', 'production', 'central'),
			('33333333-3333-3333-3333-333333333333', 'Windows Server 2', 'https://win2.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', '{\"pgVersion\": \"PostgreSQL 13.7 on x86_64-pc-windows-msvc, compiled by Visual C++ build 1914\"}'::jsonb, NOW()),
			('22222222-2222-2222-2222-222222222222', '1.0.0', '{\"pgVersion\": \"PostgreSQL 17.2, (x86_64-pc-linux-gnu, compiled by gcc)\"}'::jsonb, NOW()),
			('33333333-3333-3333-3333-333333333333', '1.0.0', '{\"pgVersion\": \"PostgreSQL 17.6 on x86_64-windows, compiled by msvc-19.44.35213, 64-bit\"}'::jsonb, NOW())"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 3);

		let win_server = servers.iter().find(|s| s.server.name.as_deref() == Some("Windows Server")).unwrap();
		let linux_server = servers.iter().find(|s| s.server.name.as_deref() == Some("Linux Server")).unwrap();
		let win2_server = servers.iter().find(|s| s.server.name.as_deref() == Some("Windows Server 2")).unwrap();

		assert_eq!(win_server.platform, Some("Windows".to_string()));
		assert_eq!(linux_server.platform, Some("Linux".to_string()));
		assert_eq!(win2_server.platform, Some("Windows".to_string()));
		assert_eq!(win_server.postgres, Some("13.7".to_string()));
		assert_eq!(linux_server.postgres, Some("17.2".to_string()));
		assert_eq!(win2_server.postgres, Some("17.6".to_string()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_mixed_server_ranks() {
	commons_tests::server::run(async |mut conn, _, private| {
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
async fn status_json_unnamed_servers_excluded() {
	commons_tests::server::run(async |mut conn, _, private| {
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

#[tokio::test(flavor = "multi_thread")]
async fn status_json_blip_status() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Blip Server', 'https://blip.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', NOW() - INTERVAL '4 minutes')"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 1);

		let server = &servers[0];
		assert_eq!(server.server.name, Some("Blip Server".to_string()));
		assert_eq!(server.up, "blip");

		let since_text = server.since.as_ref().unwrap();
		assert!(since_text.contains("m"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_gone_server() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Insert server with no status (should be "gone")
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Gone Server', 'https://gone.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		let response = private.get("/$/status.json").await;
		response.assert_status_ok();

		let servers: Vec<ServerData> = response.json();
		assert_eq!(servers.len(), 1);

		let server = &servers[0];
		assert_eq!(server.server.name, Some("Gone Server".to_string()));
		assert_eq!(server.up, "gone"); // No status means "gone"
		assert!(server.status.is_none());
		assert!(server.since.is_none());
		assert!(server.platform.is_none());
		assert!(server.postgres.is_none());
	})
	.await
}
