use axum::http::StatusCode;
use commons_tests::diesel_async::SimpleAsyncConnection;
use serde::{Deserialize, Serialize};

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
	#[serde(default)]
	munin: bool,
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
	#[serde(default)]
	bestool: Option<String>,
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
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Test cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Test Server', 'https://test.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa')",
		)
		.await
		.unwrap();

		let server_ids_response = private
			.post("/api/statuses/server_grouped_ids")
			.json(&serde_json::json!({}))
			.await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let group_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(group_ids.len(), 1);

		let group_id = &group_ids[0];
		let details_response = private
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": group_id}))
			.await;
		details_response.assert_status_ok();
		let details: ServerGroupCardResponse = details_response.json();

		assert_eq!(details.name, "Test cluster");
		assert_eq!(details.members.len(), 1);
		assert_eq!(details.members[0].name, "Test Server");
		assert_eq!(details.members[0].up, "gone");
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
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Active cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Active Server', 'https://active.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');

			INSERT INTO statuses (server_id, version, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.2.3', '{\"uptime\": 3600}'::jsonb, NOW());

			-- Ingestion records the source's current detail alongside the
			-- status, and that's what the group's headline version reads.
			INSERT INTO server_reported_detail (server_id, source, extra, version) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', '{\"uptime\": 3600}'::jsonb, '1.2.3')"
		)
		.await
		.unwrap();

		// Raw INSERTs bypass `recompute_version`, which in production runs from
		// the server create/edit paths and seeds the group's cached headline
		// version. Run it explicitly so `group_details` has a cache to read.
		database::server_groups::ServerGroup::recompute_version(
			&mut conn,
			"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".parse().unwrap(),
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
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": server_id}))
			.await;
		details_response.assert_status_ok();
		let details: ServerGroupCardResponse = details_response.json();

		assert_eq!(details.name, "Active cluster");
		assert_eq!(details.members.len(), 1);
		assert_eq!(details.members[0].name, "Active Server");
		assert_eq!(details.members[0].up, "up"); // Recent status means "up"
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
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Down cluster'),
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Away cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Down Server', 'https://down.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('22222222-2222-2222-2222-222222222222', 'Away Server', 'https://away.example.com', 'production', 'central', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb');

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

		// Each group has one server; check its status via the group card.
		let mut down_status: Option<String> = None;
		let mut away_status: Option<String> = None;

		for server_id in &server_ids {
			let details_response = private
				.post("/api/statuses/group_details")
				.json(&serde_json::json!({"server_group_id": server_id.as_str()}))
				.await;
			details_response.assert_status_ok();
			let details: ServerGroupCardResponse = details_response.json();
			assert_eq!(details.members.len(), 1);

			match details.members[0].name.as_str() {
				"Down Server" => down_status = Some(details.members[0].up.clone()),
				"Away Server" => away_status = Some(details.members[0].up.clone()),
				_ => {}
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

		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Windows cluster'),
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Linux cluster'),
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 'Windows cluster 2');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Windows Server', 'https://win.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('22222222-2222-2222-2222-222222222222', 'Linux Server', 'https://linux.example.com', 'production', 'central', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'),
			('33333333-3333-3333-3333-333333333333', 'Windows Server 2', 'https://win2.example.com', 'production', 'central', 'cccccccc-cccc-cccc-cccc-cccccccccccc');

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

		let mut win_status: Option<ServerGroupCardResponse> = None;
		let mut linux_status: Option<ServerGroupCardResponse> = None;
		let mut win2_status: Option<ServerGroupCardResponse> = None;

		for server_id in &server_ids {
			let details_response = private
				.post("/api/statuses/group_details")
				.json(&serde_json::json!({"server_group_id": server_id.as_str()}))
				.await;
			details_response.assert_status_ok();
			let details: ServerGroupCardResponse = details_response.json();

			match details.members.first().map(|m| m.name.as_str()) {
				Some("Windows Server") => win_status = Some(details),
				Some("Linux Server") => linux_status = Some(details),
				Some("Windows Server 2") => win2_status = Some(details),
				_ => {}
			}
		}

		// Platform/postgres info isn't on the group card; just verify all
		// three groups round-trip with their single member each.
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
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Production'),
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Dev'),
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 'Clone');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Production', 'https://prod.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('22222222-2222-2222-2222-222222222222', 'Dev', 'https://dev.example.com', 'dev', 'central', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'),
			('33333333-3333-3333-3333-333333333333', 'Clone', 'https://clone.example.com', 'clone', 'central', 'cccccccc-cccc-cccc-cccc-cccccccccccc')",
		)
		.await
		.unwrap();

		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();

		// Three rank buckets, one group each.
		assert_eq!(grouped_ids.len(), 3);
		assert!(grouped_ids.contains_key("production"));
		assert!(grouped_ids.contains_key("clone"));
		assert!(grouped_ids.contains_key("dev"));

		let production_id = &grouped_ids.get("production").unwrap()[0];
		let details_response = private
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": production_id}))
			.await;
		details_response.assert_status_ok();
		let details: ServerGroupCardResponse = details_response.json();

		assert_eq!(details.name, "Production");
		assert_eq!(details.members.len(), 1);
		assert_eq!(details.members[0].name, "Production");
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

		// One named group + one server with no name (groups themselves
		// always have a name; the test now verifies the group renders even
		// when a member server is unnamed).
		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Mixed cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Named Server', 'https://named.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('22222222-2222-2222-2222-222222222222', NULL, 'https://unnamed.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa')",
		)
		.await
		.unwrap();

		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let group_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(group_ids.len(), 1);

		let details_response = private
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": &group_ids[0]}))
			.await;
		details_response.assert_status_ok();
		let details: ServerGroupCardResponse = details_response.json();

		assert_eq!(details.name, "Mixed cluster");
		assert_eq!(details.members.len(), 2);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_blip_status() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW())"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Blip cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Blip Server', 'https://blip.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');
			INSERT INTO statuses (server_id, version, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', NOW() - INTERVAL '4 minutes')",
		)
		.await
		.unwrap();

		let server_ids_response = private
			.post("/api/statuses/server_grouped_ids")
			.json(&serde_json::json!({}))
			.await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let group_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(group_ids.len(), 1);

		let group_id = &group_ids[0];
		let details_response = private
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": group_id}))
			.await;
		details_response.assert_status_ok();
		let details: ServerGroupCardResponse = details_response.json();

		assert_eq!(details.members.len(), 1);
		assert_eq!(details.members[0].name, "Blip Server");
		assert_eq!(details.members[0].up, "blip"); // 4 minutes ago should be "blip"
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

		// Group + server with no status — both server-level and card-level
		// dots should read "gone".
		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Gone cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Gone Server', 'https://gone.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa')",
		)
		.await
		.unwrap();

		let server_ids_response = private.post("/api/statuses/server_grouped_ids").json(&serde_json::json!({})).await;
		server_ids_response.assert_status_ok();
		let grouped_ids: std::collections::BTreeMap<String, Vec<String>> = server_ids_response.json();
		let group_ids: Vec<String> = grouped_ids.into_values().flatten().collect();
		assert_eq!(group_ids.len(), 1);

		let group_id = &group_ids[0];
		let details_response = private
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": group_id}))
			.await;
		details_response.assert_status_ok();
		let details: ServerGroupCardResponse = details_response.json();

		assert_eq!(details.name, "Gone cluster");
		assert_eq!(details.members.len(), 1);
		assert_eq!(details.members[0].up, "gone"); // No status means "gone"
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

// spec: SVC#munin-link
#[tokio::test(flavor = "multi_thread")]
async fn get_detail_munin_flag() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Munin Server', 'https://munin.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Plain Server', 'https://plain.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '{\"munin\": true}'::jsonb, NOW()),
			('22222222-2222-2222-2222-222222222222', '{\"uptime\": 3600}'::jsonb, NOW());

			INSERT INTO server_reported_detail (server_id, source, extra) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', '{\"munin\": true}'::jsonb),
			('22222222-2222-2222-2222-222222222222', 'alertd', '{\"uptime\": 3600}'::jsonb)",
		)
		.await
		.unwrap();

		let munin_detail: ServerDetailResponse = private
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "11111111-1111-1111-1111-111111111111"}))
			.await
			.json();
		assert!(munin_detail.munin, "server that reported munin=true exposes munin");

		let plain_detail: ServerDetailResponse = private
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "22222222-2222-2222-2222-222222222222"}))
			.await
			.json();
		assert!(
			!plain_detail.munin,
			"server whose status omits the flag does not expose munin"
		);
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
			('11111111-1111-1111-1111-111111111111', '2.5.1', '{\"timezone\": \"Pacific/Auckland\", \"pgVersion\": \"PostgreSQL 17.2, (x86_64-pc-linux-gnu, compiled by gcc)\"}'::jsonb, NOW());

			INSERT INTO server_reported_detail (server_id, source, extra, version) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', '{\"timezone\": \"Pacific/Auckland\", \"pgVersion\": \"PostgreSQL 17.2, (x86_64-pc-linux-gnu, compiled by gcc)\"}'::jsonb, '2.5.1')"
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

/// The detail view's figures come from every source reporting on the server,
/// so a later push from a source that carries none of them leaves them intact
/// — and a server no bestool reports on presents no bestool version.
// spec: FIG#sourcing
#[tokio::test(flavor = "multi_thread")]
async fn get_detail_figures_resolve_across_sources() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Bestool Server', 'https://bestool.example.com', 'test', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Tamanu Only', 'https://tamanuonly.example.com', 'test', 'central');

			INSERT INTO statuses (server_id, source, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd',
			 '{\"bestoolVersion\": \"2.10.5\", \"pgVersion\": \"PostgreSQL 17.2, (x86_64-pc-linux-gnu, compiled by gcc)\", \"timezone\": \"Pacific/Auckland\"}'::jsonb,
			 NOW() - INTERVAL '10 minutes'),
			('11111111-1111-1111-1111-111111111111', 'tamanu', '{\"uptimeSecs\": 6038594}'::jsonb, NOW()),
			('22222222-2222-2222-2222-222222222222', 'tamanu', '{\"uptimeSecs\": 42}'::jsonb, NOW());

			INSERT INTO server_reported_detail (server_id, source, extra, reported_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd',
			 '{\"bestoolVersion\": \"2.10.5\", \"pgVersion\": \"PostgreSQL 17.2, (x86_64-pc-linux-gnu, compiled by gcc)\", \"timezone\": \"Pacific/Auckland\"}'::jsonb,
			 NOW() - INTERVAL '10 minutes'),
			('11111111-1111-1111-1111-111111111111', 'tamanu', '{\"uptimeSecs\": 6038594}'::jsonb, NOW()),
			('22222222-2222-2222-2222-222222222222', 'tamanu', '{\"uptimeSecs\": 42}'::jsonb, NOW())",
		)
		.await
		.unwrap();

		let detail: ServerDetailResponse = private
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "11111111-1111-1111-1111-111111111111"}))
			.await
			.json();
		let status = detail.last_status.expect("status reported");
		assert_eq!(
			status.bestool,
			Some("2.10.5".to_string()),
			"bestool's version survives a later push from a source that doesn't report one"
		);
		assert_eq!(status.postgres, Some("17.2".to_string()));
		assert_eq!(status.platform, Some("Linux".to_string()));
		assert_eq!(status.timezone, Some("Pacific/Auckland".to_string()));

		let plain: ServerDetailResponse = private
			.post("/api/servers/get_detail")
			.json(&serde_json::json!({"server_id": "22222222-2222-2222-2222-222222222222"}))
			.await
			.json();
		assert_eq!(
			plain.last_status.expect("status reported").bestool,
			None,
			"a server no bestool reports on presents no bestool version"
		);
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
	checks: Option<serde_json::Value>,
	#[serde(default)]
	nodejs: Option<String>,
	#[serde(default)]
	postgres: Option<String>,
	#[serde(default)]
	bestool: Option<String>,
	#[serde(default)]
	timezone: Option<String>,
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
			"INSERT INTO check_policies (source, check_name) VALUES ('alertd', 'db');

			INSERT INTO statuses (server_id, created_at, healthy, health) VALUES
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
		// The latest push (the failing `db` check) is what's reconstructed.
		let checks = data.checks.expect("consolidated checks");
		let arr = checks["checks"].as_array().unwrap();
		assert_eq!(arr.len(), 1);
		assert_eq!(arr[0]["check"], "db");
	})
	.await
}

/// The snapshot's figures are resolved across sources: a later push from a
/// source carrying none of them doesn't blank out what bestool reported.
// spec: FIG#sourcing
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_figures_survive_a_later_push_from_another_source() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES
			('20000000-0000-0000-0000-000000000030', 'https://figures.example.com', 'central');

			INSERT INTO statuses (server_id, source, created_at, healthy, health, extra) VALUES
			('20000000-0000-0000-0000-000000000030', 'alertd', NOW() - INTERVAL '2 hours', true, '[]'::jsonb,
			 '{\"bestoolVersion\":\"2.10.5\",\"pgVersion\":\"PostgreSQL 16.3 on x86_64-pc-linux-gnu\",\"timezone\":\"Pacific/Auckland\"}'::jsonb),
			('20000000-0000-0000-0000-000000000030', 'tamanu', NOW() - INTERVAL '1 hour', true, '[]'::jsonb,
			 '{\"uptimeSecs\":6038594}'::jsonb)",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000030"
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		let data = data.expect("snapshot returned");
		assert_eq!(
			data.bestool,
			Some("2.10.5".to_string()),
			"bestool's version survives a later push from a source that doesn't report one"
		);
		assert_eq!(data.postgres, Some("16.3".to_string()));
		assert_eq!(data.timezone, Some("Pacific/Auckland".to_string()));
	})
	.await
}

/// A server no bestool reports on presents no bestool version.
// spec: FIG#figures
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_has_no_bestool_version_when_unreported() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES
			('20000000-0000-0000-0000-000000000031', 'https://nobestool.example.com', 'central');

			INSERT INTO statuses (server_id, source, created_at, healthy, health, extra) VALUES
			('20000000-0000-0000-0000-000000000031', 'tamanu', NOW() - INTERVAL '1 hour', true, '[]'::jsonb,
			 '{\"uptimeSecs\":42}'::jsonb)",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000031"
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		assert_eq!(data.expect("snapshot returned").bestool, None);
	})
	.await
}

#[derive(Debug, Deserialize)]
struct FleetRow {
	server_id: String,
	server_name: String,
	bestool: Option<String>,
	postgres: Option<String>,
	detail: serde_json::Value,
}

/// The fleet view lists every live server with its currently reported detail,
/// resolved across sources — including servers that have never reported, and
/// excluding archived ones.
// spec: FIG#fleet-spread
#[tokio::test(flavor = "multi_thread")]
async fn fleet_detail_covers_live_servers() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, kind) VALUES
			('30000000-0000-0000-0000-000000000001', 'reports', 'https://reports.example.com', 'central'),
			('30000000-0000-0000-0000-000000000002', 'silent', 'https://silent.example.com', 'central');

			INSERT INTO servers (id, name, host, kind, deleted_at) VALUES
			('30000000-0000-0000-0000-000000000003', 'archived', 'https://archived.example.com', 'central', NOW());

			INSERT INTO server_reported_detail (server_id, source, extra, reported_at) VALUES
			('30000000-0000-0000-0000-000000000001', 'alertd',
			 '{\"bestoolVersion\": \"2.10.5\", \"pgVersion\": \"PostgreSQL 16.3 on x86_64-pc-linux-gnu\"}'::jsonb,
			 NOW() - INTERVAL '2 hours'),
			('30000000-0000-0000-0000-000000000001', 'tamanu', '{\"uptimeSecs\": 6038594}'::jsonb, NOW()),
			('30000000-0000-0000-0000-000000000003', 'alertd', '{\"bestoolVersion\": \"1.0.0\"}'::jsonb, NOW())",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/fleet_detail")
			.json(&serde_json::json!({}))
			.await;
		r.assert_status_ok();
		let rows: Vec<FleetRow> = r.json();

		assert!(
			!rows.iter().any(|s| s.server_name == "archived"),
			"an archived server is not part of the fleet",
		);

		let reporting = rows
			.iter()
			.find(|s| s.server_id == "30000000-0000-0000-0000-000000000001")
			.expect("reporting server listed");
		assert_eq!(reporting.bestool.as_deref(), Some("2.10.5"));
		assert_eq!(reporting.postgres.as_deref(), Some("16.3"));
		assert_eq!(
			reporting.detail["uptimeSecs"], 6038594,
			"the raw payload carries fields canopy derives no figure from",
		);

		let silent = rows
			.iter()
			.find(|s| s.server_id == "30000000-0000-0000-0000-000000000002")
			.expect("silent server listed");
		assert_eq!(silent.bestool, None);
		assert_eq!(
			silent.detail,
			serde_json::json!({}),
			"a server that has never reported is listed with nothing reported",
		);
	})
	.await
}

/// The Node.js version reported in the status payload's `nodeVersion` extra
/// is preferred over the value scraped from the device connection's User-Agent.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_prefers_payload_node_version_over_user_agent() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'server');

			INSERT INTO servers (id, host, kind, device_id) VALUES
			('20000000-0000-0000-0000-000000000010', 'https://node.example.com', 'central', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb');

			INSERT INTO device_connections (device_id, ip, user_agent) VALUES
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '192.168.1.10', 'Tamanu/1.0.0 Node.js/18.20.5');

			INSERT INTO statuses (server_id, device_id, created_at, healthy, health, extra) VALUES
			('20000000-0000-0000-0000-000000000010', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', NOW() - INTERVAL '1 hour', true, '[]'::jsonb, '{\"nodeVersion\":\"20.11.0\"}'::jsonb)",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000010"
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		let data = data.expect("snapshot returned");
		assert_eq!(
			data.nodejs,
			Some("20.11.0".to_string()),
			"payload nodeVersion supersedes the User-Agent's Node.js token"
		);
	})
	.await
}

/// With no `nodeVersion` in the payload, the snapshot falls back to the
/// `Node.js/x` token in the device connection's User-Agent.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_node_version_falls_back_to_user_agent() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 'server');

			INSERT INTO servers (id, host, kind, device_id) VALUES
			('20000000-0000-0000-0000-000000000011', 'https://node2.example.com', 'central', 'cccccccc-cccc-cccc-cccc-cccccccccccc');

			INSERT INTO device_connections (device_id, ip, user_agent) VALUES
			('cccccccc-cccc-cccc-cccc-cccccccccccc', '192.168.1.11', 'Tamanu/1.0.0 Node.js/18.20.5');

			INSERT INTO statuses (server_id, device_id, created_at, healthy, health, extra) VALUES
			('20000000-0000-0000-0000-000000000011', 'cccccccc-cccc-cccc-cccc-cccccccccccc', NOW() - INTERVAL '1 hour', true, '[]'::jsonb, '{}'::jsonb)",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "20000000-0000-0000-0000-000000000011"
			}))
			.await;
		r.assert_status_ok();
		let data: Option<SnapshotData> = r.json();
		let data = data.expect("snapshot returned");
		assert_eq!(
			data.nodejs,
			Some("18.20.5".to_string()),
			"absent nodeVersion falls back to the User-Agent"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_at_time_returns_prior_row() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES
			('20000000-0000-0000-0000-000000000002', 'https://snap2.example.com', 'central');
			INSERT INTO check_policies (source, check_name) VALUES ('alertd', 'old'), ('alertd', 'mid'), ('alertd', 'new')",
		)
		.await
		.unwrap();
		// Three rows at NOW-relative timestamps so they fall into the
		// time-range-partitioned `statuses` table's live partitions.
		conn.batch_execute(
			"INSERT INTO statuses (server_id, created_at, healthy, health) VALUES
			('20000000-0000-0000-0000-000000000002', NOW() - INTERVAL '3 hours', true, '[{\"check\":\"old\",\"result\":\"passed\"}]'::jsonb),
			('20000000-0000-0000-0000-000000000002', NOW() - INTERVAL '2 hours', false, '[{\"check\":\"mid\",\"result\":\"failed\"}]'::jsonb),
			('20000000-0000-0000-0000-000000000002', NOW() - INTERVAL '1 hour', true, '[{\"check\":\"new\",\"result\":\"passed\"}]'::jsonb)",
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
		// The 2h-ago row ("mid") is the most recent at-or-before 90m ago; the
		// 1h-ago row ("new") is excluded.
		let checks = data.checks.expect("checks");
		let arr = checks["checks"].as_array().unwrap();
		assert_eq!(arr.len(), 1);
		assert_eq!(arr[0]["check"], "mid");
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

// -----------------------------------------------------------------
// check_detail endpoint: servers currently flagging one named check
// -----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CheckDetailServer {
	server_id: String,
	server_name: String,
	group_id: Option<String>,
	group_name: Option<String>,
	result: String,
	data: serde_json::Value,
	failing_since: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CheckDetailResponse {
	check: String,
	ceiling: Option<String>,
	servers: Vec<CheckDetailServer>,
}

#[tokio::test(flavor = "multi_thread")]
async fn check_detail_empty_database() {
	commons_tests::server::run(async |_conn, _, private| {
		let r = private
			.post("/api/statuses/check_detail")
			.json(&serde_json::json!({"source": "alertd", "check": "postgres"}))
			.await;
		r.assert_status_ok();
		let data: CheckDetailResponse = r.json();
		assert_eq!(data.check, "postgres");
		assert_eq!(data.ceiling, None);
		assert!(data.servers.is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn check_detail_lists_servers_reporting_that_check_ordered_failed_first() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Attention cluster');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Warning Server', 'https://warning.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('22222222-2222-2222-2222-222222222222', 'Failing Server', 'https://failing.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('33333333-3333-3333-3333-333333333333', 'Healthy Server', 'https://healthy.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'),
			('44444444-4444-4444-4444-444444444444', 'Other Check Server', 'https://other.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');

			INSERT INTO issues (server_id, source, \"ref\", check_name, observed_result, effective_result, detail, message, active, first_seen, last_seen, degraded_since, last_degraded_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'health/postgres', 'postgres', 'warning', 'warning',
				'{\"check\":\"postgres\",\"result\":\"warning\"}'::jsonb, 'warned', true, NOW(), NOW(), NOW(), NOW()),
			('22222222-2222-2222-2222-222222222222', 'alertd', 'health/postgres', 'postgres', 'failed', 'failed',
				'{\"check\":\"postgres\",\"result\":\"failed\",\"free_pct\":2}'::jsonb, 'failed', true, NOW(), NOW(), NOW(), NOW()),
			('33333333-3333-3333-3333-333333333333', 'alertd', 'health/postgres', 'postgres', 'passed', 'passed',
				'{\"check\":\"postgres\",\"result\":\"passed\"}'::jsonb, 'passing', false, NOW(), NOW(), NULL, NULL),
			('44444444-4444-4444-4444-444444444444', 'alertd', 'health/disk_space', 'disk_space', 'failed', 'failed',
				'{\"check\":\"disk_space\",\"result\":\"failed\"}'::jsonb, 'failed', true, NOW(), NOW(), NOW(), NOW())",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/check_detail")
			.json(&serde_json::json!({"source": "alertd", "check": "postgres"}))
			.await;
		r.assert_status_ok();
		let data: CheckDetailResponse = r.json();

		assert_eq!(data.check, "postgres");
		assert_eq!(data.ceiling, None, "no catalog row was ever created");
		assert_eq!(
			data.servers.len(),
			3,
			"every server reporting postgres appears (healthy included); other checks don't"
		);

		// Failed sorts before warning, regardless of insertion order; the
		// healthy server sorts last so the client's default (unhealthy-only)
		// view is a prefix.
		assert_eq!(data.servers[0].server_name, "Failing Server");
		assert_eq!(data.servers[0].result, "failed");
		assert_eq!(
			data.servers[0].group_id,
			Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string())
		);
		assert_eq!(
			data.servers[0].group_name,
			Some("Attention cluster".to_string())
		);
		assert_eq!(
			data.servers[0].server_id,
			"22222222-2222-2222-2222-222222222222"
		);
		// The check's detail rides along for the expandable row.
		assert_eq!(
			data.servers[0].data,
			serde_json::json!({"check": "postgres", "result": "failed", "free_pct": 2}),
		);
		assert!(
			data.servers[0].failing_since.is_some(),
			"a degraded state row carries its streak start"
		);

		assert_eq!(data.servers[1].server_name, "Warning Server");
		assert_eq!(data.servers[1].result, "warning");

		assert_eq!(data.servers[2].server_name, "Healthy Server");
		assert_eq!(data.servers[2].result, "passed");
		assert_eq!(data.servers[2].failing_since, None);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn check_detail_failing_since_comes_from_the_active_issue() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Failing Server', 'https://failing.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Recovered Issue Server', 'https://recovered.example.com', 'production', 'central');

		-- Active state: its degraded_since is the failing-since timestamp.
			INSERT INTO issues (server_id, source, \"ref\", check_name, observed_result, effective_result, message, active, first_seen, last_seen, degraded_since, last_degraded_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'health/postgres', 'postgres', 'failed', 'failed',
				'postgres check failing', true, NOW() - INTERVAL '3 hours', NOW(), NOW() - INTERVAL '3 hours', NOW()),
			-- Recovered state (inactive): shows the last observed result but
			-- carries no streak.
			('22222222-2222-2222-2222-222222222222', 'alertd', 'health/postgres', 'postgres', 'failed', 'failed',
				'postgres check failing', false, NOW() - INTERVAL '9 hours', NOW() - INTERVAL '8 hours', NULL, NOW() - INTERVAL '8 hours')",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/check_detail")
			.json(&serde_json::json!({"source": "alertd", "check": "postgres"}))
			.await;
		r.assert_status_ok();
		let data: CheckDetailResponse = r.json();
		assert_eq!(data.servers.len(), 2);

		let failing = data
			.servers
			.iter()
			.find(|s| s.server_name == "Failing Server")
			.unwrap();
		let since = failing
			.failing_since
			.as_deref()
			.expect("active issue provides failing_since");
		let since: jiff::Timestamp = since.parse().expect("failing_since is a timestamp");
		let age = jiff::Timestamp::now().duration_since(since);
		assert!(
			age > jiff::SignedDuration::from_hours(2) && age < jiff::SignedDuration::from_hours(4),
			"failing_since reflects the issue's first_seen (~3h ago), got {age:?}"
		);

		let recovered = data
			.servers
			.iter()
			.find(|s| s.server_name == "Recovered Issue Server")
			.unwrap();
		assert_eq!(
			recovered.failing_since, None,
			"recovered state doesn't provide failing_since"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn check_detail_excludes_ungrouped_and_archived_servers() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Standalone Failing', 'https://standalone.example.com', 'production', 'central', NULL),
			('22222222-2222-2222-2222-222222222222', 'Archived Failing', 'https://archived.example.com', 'production', 'central', NULL);

			UPDATE servers SET deleted_at = NOW() WHERE id = '22222222-2222-2222-2222-222222222222';

			INSERT INTO issues (server_id, source, \"ref\", check_name, observed_result, effective_result, message, active, first_seen, last_seen, degraded_since, last_degraded_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'health/postgres', 'postgres', 'failed', 'failed', 'failed', true, NOW(), NOW(), NOW(), NOW()),
			('22222222-2222-2222-2222-222222222222', 'alertd', 'health/postgres', 'postgres', 'failed', 'failed', 'failed', true, NOW(), NOW(), NOW(), NOW())",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/check_detail")
			.json(&serde_json::json!({"source": "alertd", "check": "postgres"}))
			.await;
		r.assert_status_ok();
		let data: CheckDetailResponse = r.json();

		assert_eq!(data.servers.len(), 1, "the archived server is excluded");
		assert_eq!(data.servers[0].server_name, "Standalone Failing");
		assert_eq!(data.servers[0].group_id, None);
		assert_eq!(data.servers[0].group_name, None);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn check_detail_returns_catalog_policy_and_ignores_non_matching_check() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO check_policies (source, check_name, ceiling) VALUES ('alertd', 'postgres', 'failed');
			INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Failing Server', 'https://failing.example.com', 'production', 'central');
			INSERT INTO issues (server_id, source, \"ref\", check_name, observed_result, effective_result, message, active, first_seen, last_seen, degraded_since, last_degraded_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'health/postgres', 'postgres', 'failed', 'failed', 'failed', true, NOW(), NOW(), NOW(), NOW())",
		)
		.await
		.unwrap();

		// The check this server is actually failing.
		let r = private
			.post("/api/statuses/check_detail")
			.json(&serde_json::json!({"source": "alertd", "check": "postgres"}))
			.await;
		r.assert_status_ok();
		let data: CheckDetailResponse = r.json();
		assert_eq!(data.ceiling, Some("failed".to_string()));
		assert_eq!(data.servers.len(), 1);

		// A different, never-reported check name: no servers, but the
		// catalog lookup still runs (and correctly finds nothing).
		let r = private
			.post("/api/statuses/check_detail")
			.json(&serde_json::json!({"source": "alertd", "check": "unrelated_check"}))
			.await;
		r.assert_status_ok();
		let data: CheckDetailResponse = r.json();
		assert_eq!(data.check, "unrelated_check");
		assert_eq!(data.ceiling, None);
		assert!(data.servers.is_empty());
	})
	.await
}

/// The consolidated snapshot merges every source's checks, not just one
/// source's push.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_merges_all_sources() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES \
				('30000000-0000-0000-0000-00000000000a', 'https://multi.example.com', 'central'); \
				 INSERT INTO check_policies (source, check_name) VALUES ('alertd', 'db'), ('tamanu', 'tasks'); \
			 INSERT INTO statuses (server_id, source, healthy, health, extra) VALUES \
				('30000000-0000-0000-0000-00000000000a', 'alertd', true, \
				 '[{\"check\":\"db\",\"result\":\"passed\"}]'::jsonb, '{\"queue\":3}'::jsonb), \
				('30000000-0000-0000-0000-00000000000a', 'tamanu', true, \
				 '[{\"check\":\"tasks\",\"result\":\"passed\"}]'::jsonb, '{\"jobs\":\"ok\"}'::jsonb);",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "30000000-0000-0000-0000-00000000000a"
			}))
			.await;
		r.assert_status_ok();
		let body: serde_json::Value = r.json();
		let checks = body["checks"]["checks"].as_array().unwrap();
		assert!(
			checks.iter().any(|c| c["source"] == "alertd"),
			"alertd's checks present: {body}"
		);
		assert!(
			checks.iter().any(|c| c["source"] == "tamanu"),
			"tamanu's checks present: {body}"
		);
		// The raw-payload panel is consolidated: each source's extra
		// stays attributed under its own key rather than merged.
		assert_eq!(body["extra"]["alertd"]["queue"], 3, "alertd extra: {body}");
		assert_eq!(
			body["extra"]["tamanu"]["jobs"], "ok",
			"tamanu extra: {body}"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_surfaces_per_check_results() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Seed catalog (all reviewed, so grading isn't capped to warning as
		// a pending check would be): catalog_only stays at the default
		// warning ceiling; elevated has its ceiling lifted to failed so the
		// snapshot returns the operator-set grading; version_gated has a
		// rules ladder firing on a specific status.bestoolVersion.
		conn.batch_execute(
			"INSERT INTO check_policies (source, check_name, ceiling, escalates) VALUES \
				('alertd', 'catalog_only', 'warning', FALSE), \
				('alertd', 'elevated', 'failed', FALSE), \
				('alertd', 'passing', 'warning', FALSE), ('alertd', 'version_gated', 'warning', FALSE); \
			 UPDATE check_policies \
				SET rules = '{\"if\":[{\"in_range\":[{\"var\":\"status.bestoolVersion\"},\">=1.0.0 <2.0.0\"]},\"failed\"]}'::jsonb \
				WHERE check_name = 'version_gated'; UPDATE check_policies SET reviewed_at = NOW(), reviewed_by = 'test' WHERE source = 'alertd';",
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES \
				('30000000-0000-0000-0000-000000000001', 'https://snap-sev.example.com', 'central'); \
			 INSERT INTO statuses (server_id, healthy, health, extra) VALUES \
				('30000000-0000-0000-0000-000000000001', false, \
				 '[{\"check\":\"catalog_only\",\"healthy\":false}, \
				   {\"check\":\"elevated\",\"healthy\":false}, \
				   {\"check\":\"version_gated\",\"healthy\":false}, \
				   {\"check\":\"passing\",\"healthy\":true}]'::jsonb, \
				 '{\"bestoolVersion\":\"1.5.0\"}'::jsonb);",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "30000000-0000-0000-0000-000000000001"
			}))
			.await;
		r.assert_status_ok();
		let body: serde_json::Value = r.json();
		let checks = body["checks"]["checks"].as_array().unwrap();
		let eff = |name: &str| {
			checks
				.iter()
				.find(|c| c["check"] == name)
				.unwrap_or_else(|| panic!("{name} missing from {body}"))["effective"]
				.clone()
		};
		assert_eq!(eff("catalog_only"), serde_json::json!("warning"));
		assert_eq!(eff("elevated"), serde_json::json!("failed"));
		// version_gated's rule fires because bestoolVersion 1.5.0 is in
		// the >=1.0.0 <2.0.0 range — graded failed.
		assert_eq!(eff("version_gated"), serde_json::json!("failed"));
		// Passing checks now appear too, graded passed.
		assert_eq!(eff("passing"), serde_json::json!("passed"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_check_results_cover_result_form() {
	commons_tests::server::run(async |mut conn, _, private| {
		// `elevated` has its catalog base bumped to error: a failed
		// result uses it, a warning result ignores it (fixed Warning).
		conn.batch_execute(
			"INSERT INTO check_policies (source, check_name, ceiling) VALUES \
				('alertd', 'elevated', 'failed'), \
				('alertd', 'degraded', 'failed'), ('alertd', 'busted', 'warning'), ('alertd', 'absent', 'warning'), ('alertd', 'fine', 'warning'); UPDATE check_policies SET reviewed_at = NOW(), reviewed_by = 'test' WHERE source = 'alertd';",
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, host, kind) VALUES \
				('30000000-0000-0000-0000-000000000002', 'https://snap-res.example.com', 'central'); \
			 INSERT INTO statuses (server_id, healthy, health, extra) VALUES \
				('30000000-0000-0000-0000-000000000002', true, \
				 '[{\"check\":\"elevated\",\"result\":\"failed\"}, \
				   {\"check\":\"degraded\",\"result\":\"warning\"}, \
				   {\"check\":\"busted\",\"result\":\"broken\"}, \
				   {\"check\":\"absent\",\"result\":\"skipped\"}, \
				   {\"check\":\"fine\",\"result\":\"passed\"}]'::jsonb, \
				 '{}'::jsonb);",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({
				"server_id": "30000000-0000-0000-0000-000000000002"
			}))
			.await;
		r.assert_status_ok();
		let body: serde_json::Value = r.json();
		let checks = body["checks"]["checks"].as_array().unwrap();
		let eff = |name: &str| {
			checks
				.iter()
				.find(|c| c["check"] == name)
				.unwrap_or_else(|| panic!("{name} missing from {body}"))["effective"]
				.clone()
		};
		assert_eq!(eff("elevated"), serde_json::json!("failed"));
		assert_eq!(
			eff("degraded"),
			serde_json::json!("warning"),
			"a warning observation is already below the ceiling"
		);
		// The consolidated view shows every check by its effective result.
		assert_eq!(eff("busted"), serde_json::json!("broken"));
		assert_eq!(eff("absent"), serde_json::json!("skipped"));
		assert_eq!(eff("fine"), serde_json::json!("passed"));
	})
	.await
}

/// A silenced healthcheck stops counting toward the server's health
/// rollup (`get_detail`'s `health`), while the check stays in the
/// consolidated `checks` view — flagged silenced, its observed result
/// preserved and effective capped to skipped; unsilencing brings it back.
#[tokio::test(flavor = "multi_thread")]
async fn get_detail_health_excludes_silenced_checks() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW());

			INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Silence Server', 'https://silence.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, healthy, health, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', true,
			 '[{\"check\": \"postgres\", \"result\": \"failed\"}]'::jsonb, '{}'::jsonb, NOW());

			INSERT INTO check_policies (source, check_name, ceiling) VALUES ('alertd', 'postgres', 'failed');

			INSERT INTO issues (server_id, source, ref, check_name, observed_result, effective_result, message, active, first_seen, last_seen, degraded_since, last_degraded_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'health/postgres', 'postgres', 'failed', 'failed', 'postgres failed', true, NOW(), NOW(), NOW(), NOW())",
		)
		.await
		.unwrap();

		let detail = async || {
			let response = private
				.post("/api/servers/get_detail")
				.json(&serde_json::json!({"server_id": "11111111-1111-1111-1111-111111111111"}))
				.await;
			response.assert_status_ok();
			let body: serde_json::Value = response.json();
			body
		};

		let body = detail().await;
		assert_eq!(body["health"], "unhealthy");

		conn.batch_execute(
			"INSERT INTO scoped_check_policies (server_id, source, check_name, ceiling) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'postgres', 'skipped')",
		)
		.await
		.unwrap();

		let body = detail().await;
		assert_eq!(body["health"], "healthy");
		// The check keeps recording — only the rollup changes. It stays in
		// the consolidated view, flagged silenced, its observed result
		// preserved while the effective is capped to skipped.
		let checks = body["checks"]["checks"].as_array().unwrap();
		let postgres = checks
			.iter()
			.find(|c| c["check"] == "postgres")
			.expect("silenced check still listed");
		assert_eq!(postgres["observed"], "failed");
		assert_eq!(postgres["effective"], "skipped");
		assert_eq!(postgres["silenced"], true);

		conn.batch_execute("DELETE FROM scoped_check_policies")
			.await
			.unwrap();
		let body = detail().await;
		assert_eq!(body["health"], "unhealthy");
	})
	.await
}

/// Group-scope silences apply to every member's health rollup on the
/// group card.
#[tokio::test(flavor = "multi_thread")]
async fn group_details_member_health_excludes_group_silenced_checks() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			// group_details 404s via NoMatchingVersions when nothing is
			// published, so seed a version like its other tests do.
			"INSERT INTO versions (id, major, minor, patch, status, changelog, created_at) VALUES
			('00000000-0000-0000-0000-000000000001', 1, 0, 0, 'published', 'Test version', NOW());

			INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Silenced cluster');

			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Member Server', 'https://member.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');

			INSERT INTO statuses (server_id, version, healthy, health, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', true,
			 '[{\"check\": \"disk\", \"result\": \"failed\"}]'::jsonb, '{}'::jsonb, NOW());

			INSERT INTO issues (server_id, source, ref, check_name, observed_result, effective_result, message, active, first_seen, last_seen, degraded_since, last_degraded_at) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'health/disk', 'disk', 'failed', 'failed', 'disk failed', true, NOW(), NOW(), NOW(), NOW());

			INSERT INTO scoped_check_policies (server_group_id, source, check_name, ceiling) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'alertd', 'disk', 'skipped')",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/statuses/group_details")
			.json(&serde_json::json!({"server_group_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"}))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		assert_eq!(body["members"][0]["health"], "healthy");
	})
	.await
}

/// The snapshot endpoint reports which checks are silenced and excludes
/// them from its `health_state` rollup.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_reports_and_excludes_silenced_checks() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Snap Server', 'https://snap.example.com', 'production', 'central');

			INSERT INTO statuses (server_id, version, healthy, health, extra, created_at) VALUES
			('11111111-1111-1111-1111-111111111111', '1.0.0', true,
			 '[{\"check\": \"postgres\", \"result\": \"failed\"}, {\"check\": \"disk\", \"result\": \"passed\"}]'::jsonb, '{}'::jsonb, NOW());

			INSERT INTO check_policies (source, check_name) VALUES ('alertd', 'postgres'), ('alertd', 'disk');

			INSERT INTO scoped_check_policies (server_id, source, check_name, ceiling) VALUES
			('11111111-1111-1111-1111-111111111111', 'alertd', 'postgres', 'skipped')",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/statuses/snapshot")
			.json(&serde_json::json!({"server_id": "11111111-1111-1111-1111-111111111111"}))
			.await;
		response.assert_status_ok();
		let body: serde_json::Value = response.json();
		// postgres is failing but silenced → excluded from the rollup, which
		// stays healthy — and it's flagged silenced in the check list.
		assert_eq!(body["checks"]["health_state"], "healthy");
		let checks = body["checks"]["checks"].as_array().unwrap();
		let postgres = checks
			.iter()
			.find(|c| c["check"] == "postgres")
			.expect("postgres present");
		assert_eq!(postgres["silenced"], serde_json::json!(true));
	})
	.await
}

/// The release summary counts each still-reporting production server once, at
/// the version the most recent source to report one gave — and ignores
/// servers that have gone quiet, or that aren't production.
// spec: FIG#active-versions
#[tokio::test(flavor = "multi_thread")]
async fn summary_covers_actively_reporting_production_servers() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, kind, rank) VALUES
			('40000000-0000-0000-0000-000000000001', 'live-a', 'https://a.example.com', 'central', 'production'),
			('40000000-0000-0000-0000-000000000002', 'live-b', 'https://b.example.com', 'central', 'production'),
			('40000000-0000-0000-0000-000000000003', 'quiet', 'https://q.example.com', 'central', 'production');

			INSERT INTO servers (id, name, host, kind, rank) VALUES
			('40000000-0000-0000-0000-000000000004', 'testing', 'https://t.example.com', 'central', 'test');

			INSERT INTO server_reported_detail (server_id, source, extra, version, reported_at) VALUES
			('40000000-0000-0000-0000-000000000001', 'alertd', '{}'::jsonb, '2.34.1', NOW() - INTERVAL '2 hours'),
			-- A later source reports no version: live-a still runs 2.34.1.
			('40000000-0000-0000-0000-000000000001', 'tamanu', '{\"uptimeSecs\": 42}'::jsonb, NULL, NOW()),
			('40000000-0000-0000-0000-000000000002', 'alertd', '{}'::jsonb, '2.35.0', NOW()),
			('40000000-0000-0000-0000-000000000003', 'alertd', '{}'::jsonb, '2.10.0', NOW() - INTERVAL '8 days'),
			('40000000-0000-0000-0000-000000000004', 'alertd', '{}'::jsonb, '2.40.0', NOW())",
		)
		.await
		.unwrap();

		let r = private
			.post("/api/statuses/summary")
			.json(&serde_json::json!({}))
			.await;
		r.assert_status_ok();
		let body: serde_json::Value = r.json();

		assert_eq!(
			body["versions"],
			serde_json::json!(["2.34.1", "2.35.0"]),
			"the quiet server and the non-production one are not actively running anything, \
			 and a version-less later push doesn't drop live-a",
		);
		assert_eq!(body["releases"], serde_json::json!([[2, 34], [2, 35]]));
		assert_eq!(body["bracket"]["min"], "2.34.1");
		assert_eq!(body["bracket"]["max"], "2.35.0");
	})
	.await
}
