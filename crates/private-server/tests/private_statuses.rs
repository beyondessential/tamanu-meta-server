use commons_tests::diesel_async::SimpleAsyncConnection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct ServerDetailsResponse {
	id: String,
	name: String,
	kind: String,
	rank: String,
	host: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerStatusResponse {
	up: String,
	updated_at: Option<String>,
	version: Option<String>,
	platform: Option<String>,
	postgres: Option<String>,
	nodejs: Option<String>,
	timezone: Option<String>,
}

#[tokio::test(flavor = "multi_thread")]
async fn status_page() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.get("/").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_empty_database() {
	commons_tests::server::run(async |_conn, _, private| {
		// Get server IDs
		let server_ids_response = private
			.post("/api/private_server/fns/statuses/server_ids")
			.await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();

		assert!(server_ids.is_empty());
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", server_id)])
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		assert_eq!(details.name, "Test Server");
		assert_eq!(details.host, "https://test.example.com/");
		assert_eq!(details.rank, "production");
		assert_eq!(details.kind, "central");

		// Get server status
		let status_response = private
			.post("/api/private_server/fns/statuses/server_status")
			.form(&[("server_id", server_id)])
			.await;
		status_response.assert_status_ok();
		let status: ServerStatusResponse = status_response.json();

		assert_eq!(status.up, "gone"); // No status means "gone"
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", server_id)])
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		assert_eq!(details.name, "Active Server");

		// Get server status
		let status_response = private
			.post("/api/private_server/fns/statuses/server_status")
			.form(&[("server_id", server_id)])
			.await;
		status_response.assert_status_ok();
		let status: ServerStatusResponse = status_response.json();

		assert_eq!(status.up, "up"); // Recent status means "up"
		assert!(status.updated_at.is_some());
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 2);

		// Get status for each server
		let mut down_status = None;
		let mut away_status = None;

		for server_id in &server_ids {
			let details_response = private
				.post("/api/private_server/fns/statuses/server_details")
				.form(&[("server_id", server_id.as_str())])
				.await;
			details_response.assert_status_ok();
			let details: ServerDetailsResponse = details_response.json();

			let status_response = private
				.post("/api/private_server/fns/statuses/server_status")
				.form(&[("server_id", server_id.as_str())])
				.await;
			status_response.assert_status_ok();
			let status: ServerStatusResponse = status_response.json();

			if details.name == "Down Server" {
				down_status = Some(status);
			} else if details.name == "Away Server" {
				away_status = Some(status);
			}
		}

		assert_eq!(down_status.unwrap().up, "down"); // 45 minutes ago
		assert_eq!(away_status.unwrap().up, "away"); // 15 minutes ago
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 3);

		// Get status for each server
		let mut win_status = None;
		let mut linux_status = None;
		let mut win2_status = None;

		for server_id in &server_ids {
			let details_response = private
				.post("/api/private_server/fns/statuses/server_details")
				.form(&[("server_id", server_id.as_str())])
				.await;
			details_response.assert_status_ok();
			let details: ServerDetailsResponse = details_response.json();

			let status_response = private
				.post("/api/private_server/fns/statuses/server_status")
				.form(&[("server_id", server_id.as_str())])
				.await;
			status_response.assert_status_ok();
			let status: ServerStatusResponse = status_response.json();

			if details.name == "Windows Server" {
				win_status = Some(status);
			} else if details.name == "Linux Server" {
				linux_status = Some(status);
			} else if details.name == "Windows Server 2" {
				win2_status = Some(status);
			}
		}

		let win_status = win_status.unwrap();
		let linux_status = linux_status.unwrap();
		let win2_status = win2_status.unwrap();

		assert_eq!(win_status.platform, Some("Windows".to_string()));
		assert_eq!(linux_status.platform, Some("Linux".to_string()));
		assert_eq!(win2_status.platform, Some("Windows".to_string()));
		assert_eq!(win_status.postgres, Some("13.7".to_string()));
		assert_eq!(linux_status.postgres, Some("17.2".to_string()));
		assert_eq!(win2_status.postgres, Some("17.6".to_string()));
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 3);

		// Get first server details (should be production due to ordering)
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", &server_ids[0])])
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		assert_eq!(details.name, "Production Server");
		assert_eq!(details.rank, "production");
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 1);

		// Get server details
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", &server_ids[0])])
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		assert_eq!(details.name, "Named Server");
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", server_id)])
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		assert_eq!(details.name, "Blip Server");

		// Get server status
		let status_response = private
			.post("/api/private_server/fns/statuses/server_status")
			.form(&[("server_id", server_id)])
			.await;
		status_response.assert_status_ok();
		let status: ServerStatusResponse = status_response.json();

		assert_eq!(status.up, "blip");
		assert!(status.updated_at.is_some());
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

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_ids").await;
		server_ids_response.assert_status_ok();
		let server_ids: Vec<String> = server_ids_response.json();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", server_id)])
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		assert_eq!(details.name, "Gone Server");

		// Get server status
		let status_response = private
			.post("/api/private_server/fns/statuses/server_status")
			.form(&[("server_id", server_id)])
			.await;
		status_response.assert_status_ok();
		let status: ServerStatusResponse = status_response.json();

		assert_eq!(status.up, "gone"); // No status means "gone"
		assert!(status.updated_at.is_none());
		assert!(status.platform.is_none());
		assert!(status.postgres.is_none());
	})
	.await
}
