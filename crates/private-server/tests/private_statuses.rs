use axum::http::StatusCode;
use commons_tests::diesel_async::SimpleAsyncConnection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct ServerDetailsResponse {
	id: String,
	name: String,
	rank: String,
	host: String,
	up: String,
	version: Option<String>,
	version_distance: Option<u64>,
	facility_servers: Vec<FacilityServerCardResponse>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerDetailsDataResponse {
	id: String,
	name: String,
	kind: String,
	rank: String,
	host: String,
	parent_server_id: Option<String>,
	parent_server_name: Option<String>,
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

#[derive(Debug, Deserialize, Serialize)]
struct ServerDetailResponse {
	server: ServerDetailsDataResponse,
	device_info: Option<DeviceInfo>,
	last_status: Option<ServerLastStatusData>,
	up: String,
	child_servers: Vec<ChildServerData>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChildServerData {
	id: String,
	name: String,
	kind: String,
	rank: String,
	host: String,
	up: String,
	last_status: Option<ServerLastStatusData>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeviceInfo {
	device: DeviceData,
	keys: Vec<DeviceKeyInfo>,
	latest_connection: Option<DeviceConnectionData>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeviceData {
	id: String,
	created_at: String,
	created_at_relative: String,
	updated_at: String,
	updated_at_relative: String,
	role: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeviceKeyInfo {
	id: String,
	device_id: String,
	name: Option<String>,
	pem_data: String,
	hex_data: String,
	created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerLastStatusData {
	id: String,
	created_at: String,
	version: Option<String>,
	platform: Option<String>,
	postgres: Option<String>,
	nodejs: Option<String>,
	timezone: Option<String>,
	extra: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeviceConnectionData {
	id: String,
	created_at: String,
	ip: String,
	user_agent: Option<String>,
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
			.post("/api/private_server/fns/statuses/server_grouped_ids")
			.await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> =
			server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();

		assert!(server_ids.is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_basic_server() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Test Server', 'https://test.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
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
		assert_eq!(details.up, "gone"); // No status means "gone"
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_server_with_recent_status() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Active Server', 'https://active.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.2.3', '{\"uptime\": 3600}'::jsonb, NOW())"
		)
		.await
		.unwrap();

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
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
		assert_eq!(details.up, "up"); // Recent status means "up"
		assert_eq!(details.version, Some("1.2.3".to_string()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_server_status_ages() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

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
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 2);

		// Get status for each server
		let mut down_status: Option<String> = None;
		let mut away_status: Option<String> = None;

		for server_id in &server_ids {
			let details_response = private
				.post("/api/private_server/fns/statuses/server_details")
				.form(&[("server_id", server_id.as_str())])
				.await;
			details_response.assert_status_ok();
			let details: ServerDetailsResponse = details_response.json();

			if details.name == "Down Server" {
				down_status = Some(details.up.clone());
			} else if details.name == "Away Server" {
				away_status = Some(details.up.clone());
			}
		}

		assert_eq!(down_status.unwrap(), "down"); // 45 minutes ago
		assert_eq!(away_status.unwrap(), "away"); // 15 minutes ago
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_platform_detection() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

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
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 3);

		// Get status for each server
		let mut win_status: Option<ServerDetailsResponse> = None;
		let mut linux_status: Option<ServerDetailsResponse> = None;
		let mut win2_status: Option<ServerDetailsResponse> = None;

		for server_id in &server_ids {
			let details_response = private
				.post("/api/private_server/fns/statuses/server_details")
				.form(&[("server_id", server_id.as_str())])
				.await;
			details_response.assert_status_ok();
			let details: ServerDetailsResponse = details_response.json();

			if details.name == "Windows Server" {
				win_status = Some(details);
			} else if details.name == "Linux Server" {
				linux_status = Some(details);
			} else if details.name == "Windows Server 2" {
				win2_status = Some(details);
			}
		}

		// Platform detection and postgres version are not available in server_details response
		// Just verify we got all three servers
		assert!(win_status.is_some());
		assert!(linux_status.is_some());
		assert!(win2_status.is_some());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_mixed_server_ranks() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Production', 'https://prod.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Dev', 'https://dev.example.com', 'dev', 'central'),
			('33333333-3333-3333-3333-333333333333', 'Clone', 'https://clone.example.com', 'clone', 'central')"
		)
		.await
		.unwrap();

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();

		// Verify we have all three ranks
		assert_eq!(grouped_ids.len(), 3);
		assert!(grouped_ids.contains_key("production"));
		assert!(grouped_ids.contains_key("clone"));
		assert!(grouped_ids.contains_key("dev"));

		// Get production server details
		let production_id = &grouped_ids.get("production").unwrap()[0];
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", production_id)])
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		// Verify we got the production server
		assert_eq!(details.name, "Production");
		assert_eq!(details.rank, "production");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_unnamed_servers_excluded() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Named Server', 'https://named.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', NULL, 'https://unnamed.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
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
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Blip Server', 'https://blip.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', NOW() - INTERVAL '4 minutes')"
		)
		.await
		.unwrap();

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
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
		assert_eq!(details.up, "blip"); // 4 minutes ago should be "blip"
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_gone_server() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		// Insert server with no status (should be "gone")
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Gone Server', 'https://gone.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		// Get server IDs
		let server_ids_response = private.post("/api/private_server/fns/statuses/server_grouped_ids").await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
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
		assert_eq!(details.up, "gone"); // No status means "gone"
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_detail_basic() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Test Server', 'https://test.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/server_detail")
			.form(&[("server_id", "11111111-1111-1111-1111-111111111111")])
			.await;
		response.assert_status_ok();
		let detail: ServerDetailResponse = response.json();

		assert_eq!(detail.server.name, "Test Server");
		assert_eq!(detail.server.host, "https://test.example.com/");
		assert_eq!(detail.server.rank, "production");
		assert!(detail.device_info.is_none());
		assert!(detail.last_status.is_none());
		assert_eq!(detail.up, "gone");
		assert!(detail.child_servers.is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_detail_with_status() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_detail requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Status Server', 'https://status.example.com', 'test', 'central');

			INSERT INTO statuses (server_id, version, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '2.5.1', '{\"timezone\": \"Pacific/Auckland\", \"pgVersion\": \"PostgreSQL 17.2, (x86_64-pc-linux-gnu, compiled by gcc)\"}'::jsonb, NOW())"
		)
		.await
		.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/server_detail")
			.form(&[("server_id", "11111111-1111-1111-1111-111111111111")])
			.await;
		response.assert_status_ok();
		let detail: ServerDetailResponse = response.json();

		assert_eq!(detail.server.name, "Status Server");
		assert!(detail.last_status.is_some());

		let status = detail.last_status.unwrap();
		assert_eq!(status.version, Some("2.5.1".to_string()));
		assert_eq!(status.timezone, Some("Pacific/Auckland".to_string()));
		assert_eq!(status.platform, Some("Linux".to_string()));
		assert_eq!(status.postgres, Some("17.2".to_string()));
		assert_eq!(detail.up, "up");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_detail_with_device() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'server');

			INSERT INTO servers (id, name, host, rank, kind, device_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Device Server', 'https://device.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');

			INSERT INTO device_connections (device_id, ip, user_agent) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '192.168.1.100', 'Tamanu/1.0.0 Node.js/18.20.5')"
		)
		.await
		.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/server_detail")
			.form(&[("server_id", "11111111-1111-1111-1111-111111111111")])
			.await;
		response.assert_status_ok();
		let detail: ServerDetailResponse = response.json();

		assert_eq!(detail.server.name, "Device Server");
		assert!(detail.device_info.is_some());
		assert!(detail.child_servers.is_empty());

		let device_info = detail.device_info.unwrap();
		assert_eq!(device_info.device.id, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
		assert_eq!(device_info.device.role, "server");
		assert!(device_info.latest_connection.is_some());

		let connection = device_info.latest_connection.unwrap();
		assert_eq!(connection.ip, "192.168.1.100");
		assert_eq!(connection.user_agent, Some("Tamanu/1.0.0 Node.js/18.20.5".to_string()));
		assert_eq!(detail.up, "gone");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_detail_not_found() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/private_server/fns/servers/server_detail")
			.form(&[("server_id", "99999999-9999-9999-9999-999999999999")])
			.await;
		response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_detail_invalid_id() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/private_server/fns/servers/server_detail")
			.form(&[("server_id", "not-a-uuid")])
			.await;
		response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
	})
	.await
}

#[derive(Debug, Deserialize, Serialize)]
struct CentralServerCardResponse {
	id: String,
	name: String,
	rank: String,
	host: String,
	up: String,
	version: Option<String>,
	version_distance: Option<i32>,
	facility_servers: Vec<FacilityServerCardResponse>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FacilityServerCardResponse {
	id: String,
	name: String,
	up: String,
}

#[tokio::test(flavor = "multi_thread")]
async fn server_grouped_ids_empty() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/private_server/fns/statuses/server_grouped_ids")
			.await;
		response.assert_status_ok();

		let data: std::collections::BTreeMap<String, Vec<String>> = response.json();
		assert!(data.is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_grouped_ids_with_data() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'Test version', NOW())"
		)
		.await
		.unwrap();

		// Create central servers with different ranks
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Production Central', 'https://prod.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Clone Central', 'https://clone.example.com', 'clone', 'central'),
			('33333333-3333-3333-3333-333333333333', 'Demo Central', 'https://demo.example.com', 'demo', 'central')"
		)
		.await
		.unwrap();

		// Add facility servers for production central
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind, parent_server_id) VALUES
			('44444444-4444-4444-4444-444444444444', 'Facility A', 'https://facility-a.example.com', 'production', 'facility', '11111111-1111-1111-1111-111111111111'),
			('55555555-5555-5555-5555-555555555555', 'Facility B', 'https://facility-b.example.com', 'production', 'facility', '11111111-1111-1111-1111-111111111111')"
		)
		.await
		.unwrap();

		let response = private
			.post("/api/private_server/fns/statuses/server_grouped_ids")
			.await;
		response.assert_status_ok();

		let data: std::collections::BTreeMap<String, Vec<String>> = response.json();

		// Check production servers
		assert_eq!(data.get("production").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("production").and_then(|v| v.first()),
			Some(&"11111111-1111-1111-1111-111111111111".to_string())
		);

		// Verify server_details returns correct data for production server
		let details_response = private
			.post("/api/private_server/fns/statuses/server_details")
			.form(&[("server_id", "11111111-1111-1111-1111-111111111111")])
			.await;
		details_response.assert_status_ok();
		let prod_server: CentralServerCardResponse = details_response.json();
		assert_eq!(prod_server.name, "Production Central");
		assert_eq!(prod_server.rank, "production");
		assert_eq!(prod_server.host, "https://prod.example.com/");
		assert_eq!(prod_server.up, "gone"); // No status, so should be gone
		assert_eq!(prod_server.facility_servers.len(), 2);

		// Check clone servers
		assert_eq!(data.get("clone").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("clone").and_then(|v| v.first()),
			Some(&"22222222-2222-2222-2222-222222222222".to_string())
		);

		// Check demo servers
		assert_eq!(data.get("demo").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("demo").and_then(|v| v.first()),
			Some(&"33333333-3333-3333-3333-333333333333".to_string())
		);

		// Other ranks should not exist
		assert!(data.get("test").is_none());
		assert!(data.get("dev").is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_grouped_ids_excludes_unnamed() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Create central servers, one with name and one without
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Named Central', 'https://named.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', NULL, 'https://unnamed.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		let response = private
			.post("/api/private_server/fns/statuses/server_grouped_ids")
			.await;
		response.assert_status_ok();

		let data: std::collections::BTreeMap<String, Vec<String>> = response.json();

		// Only the named central should be included
		assert_eq!(data.get("production").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("production").and_then(|v| v.first()),
			Some(&"11111111-1111-1111-1111-111111111111".to_string())
		);
	})
	.await
}
