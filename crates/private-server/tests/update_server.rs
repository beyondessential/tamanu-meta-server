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
			.post("/api/private_server/fns/servers/update")
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
			.post("/api/private_server/fns/servers/update")
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
			.post("/api/private_server/fns/servers/update")
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
			.post("/api/private_server/fns/servers/update")
			.json(&json!({
				"server_id": "22222222-2222-2222-2222-222222222222",
				"data": {
					"rank": "invalid"
				}
			}))
			.await;
		response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
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
			.post("/api/private_server/fns/servers/update")
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
async fn update_server_parent_id() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('88888888-8888-8888-8888-888888888888', 'Parent Server', 'https://parent.example.com', 'production', 'central'),
			('99999999-9999-9999-9999-999999999999', 'Child Server', 'https://child.example.com', 'production', 'facility')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/update")
			.json(&json!({
				"server_id": "99999999-9999-9999-9999-999999999999",
				"data": {
					"parent_server_id": "88888888-8888-8888-8888-888888888888"
				}
			}))
			.await;
		response.assert_status_ok();

		let server_info = Server::get_by_id(&mut conn, "99999999-9999-9999-9999-999999999999".parse().unwrap())
			.await
			.unwrap();

		assert_eq!(server_info.parent_server_id, Some("88888888-8888-8888-8888-888888888888".parse().unwrap()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_clear_parent_id() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind, parent_server_id) VALUES
			('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Parent Server', 'https://parent2.example.com', 'production', 'central', NULL),
			('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Child Server', 'https://child2.example.com', 'production', 'facility', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/update")
			.json(&json!({
				"server_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
				"data": {
					"parent_server_id": null
				}
			}))
			.await;
		response.assert_status_ok();

		let server_info = Server::get_by_id(&mut conn, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".parse().unwrap())
			.await
			.unwrap();

		assert_eq!(server_info.parent_server_id, None);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn search_parent_by_uuid() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('cccccccc-cccc-cccc-cccc-cccccccccccc', 'Target Server', 'https://target.example.com', 'production', 'central'),
			('dddddddd-dddd-dddd-dddd-dddddddddddd', 'Current Server', 'https://current.example.com', 'production', 'facility')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/search_parent")
			.json(&json!({
				"query": "cccccccc-cccc-cccc-cccc-cccccccccccc",
				"current_server_id": "dddddddd-dddd-dddd-dddd-dddddddddddd",
				"current_rank": null,
				"current_kind": "facility"
			}))
			.await;
		response.assert_status_ok();

		let results: Vec<serde_json::Value> = response.json();
		assert_eq!(results.len(), 1);
		assert_eq!(results[0]["id"], "cccccccc-cccc-cccc-cccc-cccccccccccc");
		assert_eq!(results[0]["name"], "Target Server");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn search_parent_by_name() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee', 'Searchable Server', 'https://searchable.example.com', 'production', 'central'),
			('ffffffff-ffff-ffff-ffff-ffffffffffff', 'Current Server', 'https://current2.example.com', 'production', 'facility')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/search_parent")
			.json(&json!({
				"query": "Searchable",
				"current_server_id": "ffffffff-ffff-ffff-ffff-ffffffffffff",
				"current_rank": null,
				"current_kind": "facility"
			}))
			.await;
		response.assert_status_ok();

		let results: Vec<serde_json::Value> = response.json();
		assert_eq!(results.len(), 1);
		assert_eq!(results[0]["name"], "Searchable Server");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn search_parent_ordering_same_rank_first() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('11111111-1111-1111-1111-111111111111', 'Same Rank Server', 'https://same-rank.example.com', 'production', 'central'),
			('22222222-2222-2222-2222-222222222222', 'Different Rank Server', 'https://diff-rank.example.com', 'test', 'central'),
			('33333333-3333-3333-3333-333333333333', 'Current Server', 'https://current3.example.com', 'production', 'facility')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/search_parent")
			.json(&json!({
				"query": "Server",
				"current_server_id": "33333333-3333-3333-3333-333333333333",
				"current_rank": "production",
				"current_kind": "facility"
			}))
			.await;
		response.assert_status_ok();

		let results: Vec<serde_json::Value> = response.json();
		assert!(results.len() >= 2);
		assert_eq!(results[0]["name"], "Same Rank Server");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn search_parent_ordering_same_kind_last() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('44444444-4444-4444-4444-444444444444', 'Different Kind Server', 'https://diff-kind.example.com', 'production', 'central'),
			('55555555-5555-5555-5555-555555555555', 'Same Kind Server', 'https://same-kind.example.com', 'test', 'facility'),
			('66666666-6666-6666-6666-666666666666', 'Current Server', 'https://current4.example.com', 'production', 'facility')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/search_parent")
			.json(&json!({
				"query": "Server",
				"current_server_id": "66666666-6666-6666-6666-666666666666",
				"current_rank": "production",
				"current_kind": "facility"
			}))
			.await;
		response.assert_status_ok();

		let results: Vec<serde_json::Value> = response.json();
		assert!(results.len() >= 2);
		assert_eq!(results[0]["name"], "Different Kind Server");
		assert_eq!(results[results.len() - 1]["name"], "Same Kind Server");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn search_parent_excludes_current_server() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('77777777-7777-7777-7777-777777777777', 'Current Server', 'https://current5.example.com', 'production', 'facility')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/servers/search_parent")
			.json(&json!({
				"query": "Current",
				"current_server_id": "77777777-7777-7777-7777-777777777777",
				"current_rank": null,
				"current_kind": "facility"
			}))
			.await;
		response.assert_status_ok();

		let results: Vec<serde_json::Value> = response.json();
		assert_eq!(results.len(), 0);
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
			.post("/api/private_server/fns/servers/update")
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
		assert_eq!(server_info.host.0.to_string(), "https://updated.example.com/");
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
			.post("/api/private_server/fns/servers/update")
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
			.post("/api/private_server/fns/servers/update")
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
