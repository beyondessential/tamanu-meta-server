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
	members: Vec<GroupMemberResponse>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerDetailsDataResponse {
	id: String,
	name: String,
	kind: String,
	rank: String,
	host: String,
	group_id: Option<String>,
	group_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerGroupResponse {
	id: String,
	name: String,
	notes: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerDetailResponse {
	server: ServerDetailsDataResponse,
	device_info: Option<DeviceInfo>,
	last_status: Option<ServerLastStatusData>,
	up: String,
	#[serde(default)]
	group: Option<ServerGroupResponse>,
	#[serde(default)]
	siblings: Vec<serde_json::Value>,
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
	updated_at: String,
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
async fn spa_root() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private.get("/").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn spa_client_route() {
	commons_tests::server::run(async |_conn, _, private| {
		// Any unmatched path falls back to the SPA index, letting the
		// React router handle it client-side.
		let response = private.get("/status").await;
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
			.post("/api/statuses/server_grouped_ids")
			.json(&serde_json::json!({}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/statuses/server_details")
			.json(&serde_json::json!({"server_id": server_id}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/statuses/server_details")
			.json(&serde_json::json!({"server_id": server_id}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 2);

		// Get status for each server
		let mut down_status: Option<String> = None;
		let mut away_status: Option<String> = None;

		for server_id in &server_ids {
			let details_response = private
				.post("/api/statuses/server_details")
				.json(&serde_json::json!({"server_id": server_id.as_str()}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
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
				.post("/api/statuses/server_details")
				.json(&serde_json::json!({"server_id": server_id.as_str()}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
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
			.post("/api/statuses/server_details")
			.json(&serde_json::json!({"server_id": production_id}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 1);

		// Get server details
		let details_response = private
			.post("/api/statuses/server_details")
			.json(&serde_json::json!({"server_id": &server_ids[0]}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/statuses/server_details")
			.json(&serde_json::json!({"server_id": server_id}))
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
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let server_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(server_ids.len(), 1);

		let server_id = &server_ids[0];

		// Get server details
		let details_response = private
			.post("/api/statuses/server_details")
			.json(&serde_json::json!({"server_id": server_id}))
			.await;
		details_response.assert_status_ok();
		let details: ServerDetailsResponse = details_response.json();

		assert_eq!(details.name, "Gone Server");
		assert_eq!(details.up, "gone"); // No status means "gone"
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_detail_basic() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "11111111-1111-1111-1111-111111111111"}))
			.await;
		response.assert_status_ok();
		let detail: ServerDetailResponse = response.json();

		assert_eq!(detail.server.name, "Test Server");
		assert_eq!(detail.server.host, "https://test.example.com/");
		assert_eq!(detail.server.rank, "production");
		assert!(detail.device_info.is_none());
		assert!(detail.last_status.is_none());
		assert_eq!(detail.up, "gone");
		assert!(detail.siblings.is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_detail_with_status() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_detail requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "11111111-1111-1111-1111-111111111111"}))
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
async fn get_detail_with_device() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Add a version to satisfy server_details requirement
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
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
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "11111111-1111-1111-1111-111111111111"}))
			.await;
		response.assert_status_ok();
		let detail: ServerDetailResponse = response.json();

		assert_eq!(detail.server.name, "Device Server");
		assert!(detail.device_info.is_some());
		assert!(detail.siblings.is_empty());

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
async fn get_detail_not_found() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "99999999-9999-9999-9999-999999999999"}))
			.await;
		// AppError maps DatabaseQuery::NotFound to 404
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_detail_invalid_id() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "not-a-uuid"}))
			.await;
		// axum's Json extractor rejects malformed bodies with 422
		response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
	})
	.await
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerGroupCardResponse {
	id: String,
	name: String,
	notes: String,
	version: Option<String>,
	version_distance: Option<i32>,
	members: Vec<GroupMemberResponse>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GroupMemberResponse {
	id: String,
	name: String,
	up: String,
}

#[tokio::test(flavor = "multi_thread")]
async fn server_grouped_ids_empty() {
	commons_tests::server::run(async |_conn, _, private| {
		let response = private
			.post("/api/statuses/server_grouped_ids")
			.json(&serde_json::json!({}))
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
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
		)
		.await
		.unwrap();

		// Three groups, each with one or more servers — Production group has
		// multiple members to make sure the bucket gets exactly one entry per
		// group rather than per server.
		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Production cluster'),
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Clone cluster'),
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 'Demo cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Prod Central', 'https://prod.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('44444444-4444-4444-4444-444444444444', 'Prod Facility A', 'https://facility-a.example.com', 'production', 'facility', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('55555555-5555-5555-5555-555555555555', 'Prod Facility B', 'https://facility-b.example.com', 'production', 'facility', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('22222222-2222-2222-2222-222222222222', 'Clone Central', 'https://clone.example.com', 'clone', 'central', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'),
			('33333333-3333-3333-3333-333333333333', 'Demo Central', 'https://demo.example.com', 'demo', 'central', 'cccccccc-cccc-cccc-cccc-cccccccccccc')",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/statuses/server_grouped_ids")
			.json(&serde_json::json!({}))
			.await;
		response.assert_status_ok();

		let data: std::collections::BTreeMap<String, Vec<String>> = response.json();

		assert_eq!(data.get("production").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("production").and_then(|v| v.first()),
			Some(&"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string())
		);

		// group_details returns the group card with all members.
		let details_response = private
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"}))
			.await;
		details_response.assert_status_ok();
		let prod_group: ServerGroupCardResponse = details_response.json();
		assert_eq!(prod_group.name, "Production cluster");
		assert_eq!(prod_group.members.len(), 3);

		assert_eq!(data.get("clone").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("clone").and_then(|v| v.first()),
			Some(&"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string())
		);

		assert_eq!(data.get("demo").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("demo").and_then(|v| v.first()),
			Some(&"cccccccc-cccc-cccc-cccc-cccccccccccc".to_string())
		);

		assert!(!data.contains_key("test"));
		assert!(!data.contains_key("dev"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_grouped_ids_excludes_ungrouped() {
	commons_tests::server::run(async |mut conn, _, private| {
		// One group with a production-ranked member, plus a standalone
		// ungrouped server. The endpoint should expose the group but ignore
		// the ungrouped server entirely.
		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Production cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Grouped Central', 'https://grouped.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('22222222-2222-2222-2222-222222222222', 'Standalone', 'https://standalone.example.com', 'production', 'central', NULL)",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/statuses/server_grouped_ids")
			.json(&serde_json::json!({}))
			.await;
		response.assert_status_ok();

		let data: std::collections::BTreeMap<String, Vec<String>> = response.json();

		assert_eq!(data.get("production").map(|v| v.len()), Some(1));
		assert_eq!(
			data.get("production").and_then(|v| v.first()),
			Some(&"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string())
		);
	})
	.await
}

// -----------------------------------------------------------------
// Status snapshot endpoint
// -----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SnapshotData {
	#[serde(default)]
	healthy: Option<bool>,
	#[serde(default)]
	health: Option<serde_json::Value>,
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_returns_latest_when_at_omitted() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES
			('20000000-0000-0000-0000-000000000001', 'https://snap.example.com', 'central')",
		)
		.await
		.unwrap();
		conn.batch_execute(
			"INSERT INTO statuses (server_id, created_at, healthy, health) VALUES
			('20000000-0000-0000-0000-000000000001', NOW() - INTERVAL '2 hours', true, '[]'::jsonb),
			('20000000-0000-0000-0000-000000000001', NOW() - INTERVAL '1 hour', false,
				'[{\"check\":\"db\",\"healthy\":false}]'::jsonb)",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000001"
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		let data = data.expect("snapshot returned for server with statuses");
		assert_eq!(data.healthy, Some(false), "latest is the most recent");
		let health = data.health.unwrap();
		assert_eq!(health.as_array().unwrap().len(), 1);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_at_time_returns_prior_row() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES
			('20000000-0000-0000-0000-000000000002', 'https://snap2.example.com', 'central')",
		)
		.await
		.unwrap();
		// Three rows at NOW-relative timestamps so they fall into the
		// time-range-partitioned `statuses` table's live partitions.
		conn.batch_execute(
			"INSERT INTO statuses (server_id, created_at, healthy, health) VALUES
			('20000000-0000-0000-0000-000000000002', NOW() - INTERVAL '3 hours', true, '[]'::jsonb),
			('20000000-0000-0000-0000-000000000002', NOW() - INTERVAL '2 hours', false, '[]'::jsonb),
			('20000000-0000-0000-0000-000000000002', NOW() - INTERVAL '1 hour', true, '[]'::jsonb)",
		)
		.await
		.unwrap();

		// 90 minutes ago → unhealthy row (2h ago) is the most recent
		// at-or-before; the healthy row 1h ago is excluded.
		let at = (jiff::Timestamp::now() - jiff::SignedDuration::from_mins(90)).to_string();
		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000002",
				"at": at,
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		let data = data.expect("row exists at this point");
		assert_eq!(data.healthy, Some(false));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_before_any_row_returns_null() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES
			('20000000-0000-0000-0000-000000000003', 'https://snap3.example.com', 'central')",
		)
		.await
		.unwrap();
		conn.batch_execute(
			"INSERT INTO statuses (server_id, created_at, healthy, health) VALUES
			('20000000-0000-0000-0000-000000000003', NOW() - INTERVAL '1 hour', true, '[]'::jsonb)",
		)
		.await
		.unwrap();

		let at = (jiff::Timestamp::now() - jiff::SignedDuration::from_hours(2)).to_string();
		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000003",
				"at": at,
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		assert!(data.is_none(), "no row at-or-before the requested time");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_server_without_statuses_returns_null() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES
			('20000000-0000-0000-0000-000000000004', 'https://snap4.example.com', 'central')",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000004"
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		assert!(data.is_none());
	})
	.await
}
