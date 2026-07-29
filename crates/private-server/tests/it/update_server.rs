use axum::http::StatusCode;
use commons_tests::diesel_async::SimpleAsyncConnection;
use database::servers::Server;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn update_server_basic_fields() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('22222222-2222-2222-2222-222222222222', 'Original Server', 'https://original.example.com', 'test', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "22222222-2222-2222-2222-222222222222",
				"data": {
					"name": "Updated Server",
					"host": "https://updated.example.com",
					"rank": "production"
				}
			}))
			.await;
		response.assert_status_ok();
		// update returns Result<()>, no response body
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_partial_update() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('33333333-3333-3333-3333-333333333333', 'Partial Server', 'https://partial.example.com', 'demo', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "33333333-3333-3333-3333-333333333333",
				"data": {
					"rank": "production"
				}
			}))
			.await;
		response.assert_status_ok();
		// update returns Result<()>, no response body
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_device_id() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('44444444-4444-4444-4444-444444444444', 'server')"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('55555555-5555-5555-5555-555555555555', 'Device Server', 'https://device.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "55555555-5555-5555-5555-555555555555",
				"data": {
					"device_id": "44444444-4444-4444-4444-444444444444"
				}
			}))
			.await;
		response.assert_status_ok();
		// update returns Result<()>, no response body
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_invalid_rank() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('66666666-6666-6666-6666-666666666666', 'Rank Server', 'https://rank.example.com', 'test', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "22222222-2222-2222-2222-222222222222",
				"data": {
					"rank": "invalid"
				}
			}))
			.await;
		// axum's Json extractor rejects unknown enum variants with 422
		response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_not_found() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "77777777-7777-7777-7777-777777777777",
				"data": {}
			}))
			.await;
		response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_group_id() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('88888888-8888-8888-8888-888888888888', 'Group A');
			INSERT INTO servers (id, name, host, rank, kind) VALUES
			('99999999-9999-9999-9999-999999999999', 'Member', 'https://member.example.com', 'production', 'facility');
			INSERT INTO admins (email) VALUES ('admin@example.com')",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "99999999-9999-9999-9999-999999999999",
				"data": {
					"group_id": "88888888-8888-8888-8888-888888888888"
				}
			}))
			.await;
		response.assert_status_ok();

		let server_info =
			Server::get_by_id(&mut conn, "99999999-9999-9999-9999-999999999999".parse().unwrap())
				.await
				.unwrap();

		assert_eq!(
			server_info.group_id,
			Some("88888888-8888-8888-8888-888888888888".parse().unwrap())
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_clear_group_id() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO server_groups (id, name) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Group');
			INSERT INTO servers (id, name, host, rank, kind, group_id) VALUES
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Member', 'https://m2.example.com', 'production', 'facility', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');
			INSERT INTO admins (email) VALUES ('admin@example.com')",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
				"data": {
					"group_id": null
				}
			}))
			.await;
		response.assert_status_ok();

		let server_info =
			Server::get_by_id(&mut conn, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".parse().unwrap())
				.await
				.unwrap();

		assert_eq!(server_info.group_id, None);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_notes_and_tags() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 'Tagged Server', 'https://tagged.example.com', 'production', 'central');
			INSERT INTO admins (email) VALUES ('admin@example.com')",
		)
		.await
		.unwrap();

		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "cccccccc-cccc-cccc-cccc-cccccccccccc",
				"data": {
					"notes": "ops handover note",
					"tags": { "env": "prod", "tier": "1" }
				}
			}))
			.await;
		response.assert_status_ok();

		let server_info =
			Server::get_by_id(&mut conn, "cccccccc-cccc-cccc-cccc-cccccccccccc".parse().unwrap())
				.await
				.unwrap();
		assert_eq!(server_info.notes, "ops handover note");
		assert_eq!(server_info.tags.0.get("env"), Some(&"prod".to_string()));
		assert_eq!(server_info.tags.0.get("tier"), Some(&"1".to_string()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_preserves_device_id_when_not_provided() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Create a device and a server with that device_id
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'server')"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind, device_id) VALUES
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Device Server', 'https://device.example.com', 'production', 'central', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		// Update server without providing device_id in the update data
		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
				"data": {
					"name": "Updated Name",
					"host": "https://updated.example.com"
				}
			}))
			.await;
		response.assert_status_ok();

		// Verify the server still has the device_id
		let server_info = Server::get_by_id(&mut conn, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".parse().unwrap())
			.await
			.unwrap();

		assert_eq!(server_info.name, Some("Updated Name".to_string()));
		assert_eq!(
			server_info.host.as_ref().unwrap().0.to_string(),
			"https://updated.example.com/"
		);
		assert_eq!(server_info.device_id, Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".parse().unwrap()),
			"Device ID should still be present when not provided in update");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_clears_device_id_with_null() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Create a device and a server with that device_id
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 'server')"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind, device_id) VALUES
			('dddddddd-dddd-dddd-dddd-dddddddddddd', 'Server With Device', 'https://withdevice.example.com', 'production', 'central', 'cccccccc-cccc-cccc-cccc-cccccccccccc')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		// Update server with device_id explicitly set to null
		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "dddddddd-dddd-dddd-dddd-dddddddddddd",
				"data": {
					"name": "Server Without Device",
					"device_id": null
				}
			}))
			.await;
		response.assert_status_ok();

		// Verify the server no longer has the device_id
		let server_info = Server::get_by_id(&mut conn, "dddddddd-dddd-dddd-dddd-dddddddddddd".parse().unwrap())
			.await
			.unwrap();

		assert_eq!(server_info.name, Some("Server Without Device".to_string()));
		assert_eq!(server_info.device_id, None,
			"Device ID should be cleared when explicitly set to null in update");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_sets_new_device_id() {
	commons_tests::server::run(async |mut conn, _, private| {
		// Create two devices
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee', 'server'),
			('ffffffff-ffff-ffff-ffff-ffffffffffff', 'server')"
		)
		.await
		.unwrap();

		// Create a server with the first device
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind, device_id) VALUES
			('11111111-1111-1111-1111-111111111111', 'Original Server', 'https://original.example.com', 'production', 'central', 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		// Update server with a new device_id
		let response = private
			.post("/api/servers/update")
			.json(&json!({
				"server_id": "11111111-1111-1111-1111-111111111111",
				"data": {
					"name": "Updated Server",
					"device_id": "ffffffff-ffff-ffff-ffff-ffffffffffff"
				}
			}))
			.await;
		response.assert_status_ok();

		// Verify the server now has the new device_id
		let server_info = Server::get_by_id(&mut conn, "11111111-1111-1111-1111-111111111111".parse().unwrap())
			.await
			.unwrap();

		assert_eq!(server_info.name, Some("Updated Server".to_string()));
		assert_eq!(server_info.device_id, Some("ffffffff-ffff-ffff-ffff-ffffffffffff".parse().unwrap()),
			"Device ID should be updated to new value when provided in update");
	})
	.await
}

/// The per-server name-management grants (DOM) are withheld at creation and
/// carried by the ordinary update path, one independently of the other.
#[tokio::test(flavor = "multi_thread")]
async fn update_server_name_management_grants() {
	commons_tests::server::run(async |mut conn, _, private| {
		let id = "44444444-4444-4444-4444-444444444444";
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('{id}', 'DNS Server', 'https://dns.example.com', 'production', 'central')"
		))
		.await
		.unwrap();

		let server = Server::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert!(!server.may_manage_dns, "withheld until granted");
		assert!(!server.may_manage_tls, "withheld until granted");

		private
			.post("/api/servers/update")
			.json(&json!({"server_id": id, "data": {"may_manage_dns": true}}))
			.await
			.assert_status_ok();

		let server = Server::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert!(server.may_manage_dns);
		assert!(!server.may_manage_tls, "granting DNS must not grant TLS");

		// An update touching neither leaves both alone.
		private
			.post("/api/servers/update")
			.json(&json!({"server_id": id, "data": {"name": "Renamed"}}))
			.await
			.assert_status_ok();
		let server = Server::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert!(
			server.may_manage_dns,
			"an unrelated update must not revoke it"
		);

		// Revoked again.
		private
			.post("/api/servers/update")
			.json(
				&json!({"server_id": id, "data": {"may_manage_dns": false, "may_manage_tls": true}}),
			)
			.await
			.assert_status_ok();
		let server = Server::get_by_id(&mut conn, id.parse().unwrap())
			.await
			.unwrap();
		assert!(!server.may_manage_dns);
		assert!(server.may_manage_tls);
	})
	.await
}
