//! Integration tests for the admin attach/detach/merge endpoints on
//! `/api/devices/...`. These are the operator workflows that move
//! today's mTLS-only fleet onto the tailnet.

use axum_client_ip::ClientIpSource;
use commons_servers::{
	router,
	tailnet_directory::{DirectoryEntry, TailnetDirectory},
};
use commons_tests::axum_test::TestServer;
use commons_tests::db::TestDb;
use commons_tests::diesel_async::SimpleAsyncConnection;
use uuid::Uuid;

fn test_directory() -> (std::net::IpAddr, String, TailnetDirectory) {
	let ip: std::net::IpAddr = "100.64.0.42".parse().unwrap();
	let node_id = "nodekey:admintest42".to_string();
	let dir = TailnetDirectory::for_test([(
		ip,
		DirectoryEntry {
			node_id: node_id.clone(),
			node_name: "admin-test.example.ts.net".into(),
			tailnet: "example.ts.net".into(),
			tags: vec!["tag:canopy-server".into()],
			addresses: vec![ip],
			last_seen: None,
			key_expiry_disabled: true,
		},
	)]);
	(ip, node_id, dir)
}

async fn private_with_directory(url: &str, directory: TailnetDirectory) -> TestServer {
	let db = database::init_to(url);
	let router = router(
		private_server::routes(private_server::state::AppState {
			db: db.clone(),
			db_read: db,
			ro_pool: None,
			tailnet_directory: Some(directory),
			kube: None,
			sts: None,
			prober: private_server::backup_probe::BucketProber::fake(
				private_server::backup_probe::ProbeState::Empty,
			),
			recovery_recipients: None,
			recovery_challenge: std::sync::Arc::new(std::sync::Mutex::new(None)),
		})
		.unwrap(),
		ClientIpSource::RightmostForwarded,
	);
	let mut server = TestServer::new(router);
	// Operator's laptop, not a tailnet caller — the admin endpoints
	// are gated by `TailscaleAdmin` (debug-mode bypass in tests) and
	// the tagged-device guard must not fire.
	server.add_header("Forwarded", "for=192.0.2.10");
	server
}

async fn insert_device(conn: &mut commons_tests::diesel_async::AsyncPgConnection) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO devices (id, role) VALUES ('{id}', 'server');"
	))
	.await
	.expect("insert device");
	id
}

#[tokio::test(flavor = "multi_thread")]
async fn attach_tailscale_resolves_by_ip_and_writes_columns() {
	TestDb::run(async |mut conn, url| {
		let (ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;
		let device_id = insert_device(&mut conn).await;

		let resp = private
			.post("/api/devices/attach_tailscale")
			.json(&serde_json::json!({
				"device_id": device_id,
				"identifier": ip.to_string(),
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(
			body["device"]["tailscale_node_id"].as_str(),
			Some(node_id.as_str())
		);
		assert_eq!(
			body["tailnet_live"]["display_name"].as_str(),
			Some("admin-test.example.ts.net"),
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn attach_tailscale_resolves_by_node_id() {
	TestDb::run(async |mut conn, url| {
		let (_ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;
		let device_id = insert_device(&mut conn).await;

		let resp = private
			.post("/api/devices/attach_tailscale")
			.json(&serde_json::json!({
				"device_id": device_id,
				"identifier": node_id,
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(
			body["device"]["tailscale_node_id"].as_str(),
			Some(node_id.as_str())
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn attach_tailscale_resolves_by_dns_name() {
	TestDb::run(async |mut conn, url| {
		let (_ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;
		let device_id = insert_device(&mut conn).await;

		let resp = private
			.post("/api/devices/attach_tailscale")
			.json(&serde_json::json!({
				"device_id": device_id,
				"identifier": "admin-test",
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(
			body["device"]["tailscale_node_id"].as_str(),
			Some(node_id.as_str())
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn attach_tailscale_conflict_when_node_id_already_claimed() {
	TestDb::run(async |mut conn, url| {
		let (_ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;

		// Pre-attach the node id to one device.
		let claimer = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role, tailscale_node_id) VALUES ('{claimer}', 'server', '{node_id}');"
		))
		.await
		.expect("seed claimer");

		// Try to attach the same node id to a different device.
		let other = insert_device(&mut conn).await;
		let resp = private
			.post("/api/devices/attach_tailscale")
			.json(&serde_json::json!({
				"device_id": other,
				"identifier": node_id,
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 409);
		let body: serde_json::Value = resp.json();
		assert_eq!(
			body["type"].as_str(),
			Some("/errors/device-tailscale-node-already-claimed"),
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn attach_tailscale_conflicts_when_node_already_claimed() {
	TestDb::run(async |mut conn, url| {
		let (_ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;

		// Another device already holds the node id. There are no untrusted
		// placeholders to silently displace, so attaching it elsewhere is a
		// conflict the operator must resolve via merge.
		let claimant = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role, tailscale_node_id, tailscale_node_name) \
			 VALUES ('{claimant}', 'server', '{node_id}', 'claimant-name');"
		))
		.await
		.expect("seed claimant");

		let target = insert_device(&mut conn).await;

		let resp = private
			.post("/api/devices/attach_tailscale")
			.json(&serde_json::json!({
				"device_id": target,
				"identifier": node_id,
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 409);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn detach_tailscale_clears_columns() {
	TestDb::run(async |mut conn, url| {
		let (_ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;

		let device_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role, tailscale_node_id, tailscale_node_name) \
			 VALUES ('{device_id}', 'server', '{node_id}', 'old-name');"
		))
		.await
		.expect("seed device");

		let resp = private
			.post("/api/devices/detach_tailscale")
			.json(&serde_json::json!({ "device_id": device_id }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert!(body["device"]["tailscale_node_id"].is_null());
		assert!(body["device"]["tailscale_node_name"].is_null());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn merge_into_reparents_keys_and_deletes_source() {
	TestDb::run(async |mut conn, url| {
		let private = private_with_directory(&url, test_directory().2).await;

		let source = Uuid::new_v4();
		let target = Uuid::new_v4();
		// Source: tailnet-only, server-role.
		// Target: mTLS-only, server-role, has one key.
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role, tailscale_node_id) \
			   VALUES ('{source}', 'server', 'nodekey:fromauto'); \
			 INSERT INTO devices (id, role) VALUES ('{target}', 'server'); \
			 INSERT INTO device_keys (device_id, key_data, name, is_active) \
			   VALUES ('{target}', '\\x010203', 'mtls', true);"
		))
		.await
		.expect("seed devices");

		let resp = private
			.post("/api/devices/merge_into")
			.json(&serde_json::json!({
				"source_id": source,
				"target_id": target,
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();

		// Target should now own the tailscale identity.
		assert_eq!(
			body["device"]["id"].as_str(),
			Some(target.to_string().as_str()),
		);
		assert_eq!(
			body["device"]["tailscale_node_id"].as_str(),
			Some("nodekey:fromauto"),
		);

		// Source row gone.
		conn.batch_execute(&format!(
			"DO $$ BEGIN IF EXISTS (SELECT 1 FROM devices WHERE id = '{source}') \
			 THEN RAISE EXCEPTION 'source device should have been deleted'; END IF; END $$;"
		))
		.await
		.expect("source deleted");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn merge_into_conflict_when_both_have_tailscale() {
	TestDb::run(async |mut conn, url| {
		let private = private_with_directory(&url, test_directory().2).await;

		let source = Uuid::new_v4();
		let target = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role, tailscale_node_id) VALUES \
			   ('{source}', 'server', 'nodekey:src'), \
			   ('{target}', 'server', 'nodekey:tgt');"
		))
		.await
		.expect("seed devices with both holding tailscale");

		let resp = private
			.post("/api/devices/merge_into")
			.json(&serde_json::json!({
				"source_id": source,
				"target_id": target,
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 409);
		let body: serde_json::Value = resp.json();
		assert_eq!(body["type"].as_str(), Some("/errors/device-merge-conflict"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_tailnet_identifier_returns_match() {
	TestDb::run(async |_conn, url| {
		let (ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;

		let resp = private
			.post("/api/devices/resolve_tailnet_identifier")
			.json(&serde_json::json!({ "identifier": ip.to_string() }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["matched"]["node_id"].as_str(), Some(node_id.as_str()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_tailnet_identifier_returns_null_for_unknown() {
	TestDb::run(async |_conn, url| {
		let (_ip, _node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;

		let resp = private
			.post("/api/devices/resolve_tailnet_identifier")
			.json(&serde_json::json!({ "identifier": "100.64.99.99" }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert!(body["matched"].is_null());
	})
	.await
}
