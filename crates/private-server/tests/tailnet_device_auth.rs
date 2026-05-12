//! Integration tests for the tailnet path of the device-auth extractor
//! on the private-server's `/public/...` mount.

use commons_tests::diesel_async::SimpleAsyncConnection;
use uuid::Uuid;

async fn provision_server(
	conn: &mut commons_tests::diesel_async::AsyncPgConnection,
	device_id: Uuid,
) -> Uuid {
	let server_id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO servers (id, host, kind, device_id) \
		 VALUES ('{server_id}', 'https://test.example.com', 'central', '{device_id}');"
	))
	.await
	.expect("insert server");
	server_id
}

#[tokio::test(flavor = "multi_thread")]
async fn tailnet_server_device_can_post_event() {
	commons_tests::server::run_with_tailnet_device_auth(
		"server",
		async |mut conn, tailnet_ip, _node_id, device_id, _public, private| {
			let server_id = provision_server(&mut conn, device_id).await;

			let response = private
				.post("/public/events")
				.add_header("Forwarded", &format!("for={tailnet_ip}"))
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "disk-/var",
					"message": "less than 5% free",
				}))
				.await;
			response.assert_status_ok();

			let body: serde_json::Value = response.json();
			assert_eq!(
				body.get("server_id").and_then(|v| v.as_str()),
				Some(server_id.to_string().as_str()),
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_tailnet_node_auto_creates_untrusted_then_403s_role_gate() {
	// The directory knows the caller's IP but no `Device` row has the
	// corresponding `tailscale_node_id` yet. The dual-auth extractor
	// should auto-create the device row with role `Untrusted`, then the
	// `ServerDevice` role wrapper rejects it for insufficient permissions.
	use axum_client_ip::ClientIpSource;
	use commons_servers::{
		router,
		tailnet_directory::{DirectoryEntry, TailnetDirectory},
	};
	use commons_tests::axum_test::TestServer;
	use commons_tests::db::TestDb;

	TestDb::run(async |conn, url| {
		let tailnet_ip: std::net::IpAddr = "100.64.0.99".parse().unwrap();
		let unknown_node_id = "nodekey:firstcontact99".to_string();

		let directory = TailnetDirectory::for_test([(
			tailnet_ip,
			DirectoryEntry {
				node_id: unknown_node_id.clone(),
				node_name: "fresh-node".into(),
				tailnet: "test-tailnet".into(),
				tags: vec!["tag:canopy-server".into()],
				addresses: vec![tailnet_ip],
			},
		)]);

		let private_router = router(
			private_server::routes(private_server::state::AppState {
				db: database::init_to(&url),
				ro_pool: None,
				tailnet_directory: Some(directory),
			})
			.unwrap(),
			ClientIpSource::RightmostForwarded,
		);
		let private = TestServer::new(private_router);

		let response = private
			.post("/public/events")
			.add_header("Forwarded", &format!("for={tailnet_ip}"))
			.json(&serde_json::json!({
				"source": "watchdog",
				"ref": "x",
				"message": "first contact",
			}))
			.await;
		assert_eq!(response.status_code().as_u16(), 403);

		// And the device row should now exist with role Untrusted, ready
		// for an admin to promote.
		use commons_tests::diesel_async::SimpleAsyncConnection;
		let mut conn = conn;
		conn.batch_execute(&format!(
			"DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM devices \
			 WHERE tailscale_node_id = '{unknown_node_id}' AND role = 'untrusted') \
			 THEN RAISE EXCEPTION 'auto-discovery did not insert the expected device row'; END IF; END $$;"
		))
		.await
		.expect("untrusted device row exists after auto-discovery");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn non_tailnet_source_ip_rejected() {
	// Same setup (directory populated, device pre-attached), but the
	// request comes in with a public-IP Forwarded header. The tailnet
	// path's spoof-guard rejects on `!is_tailnet_ip`, and the tunnel
	// path doesn't fall back to mTLS (the Tailscale ingress proxy
	// terminates client TLS, so attempting mTLS here would never
	// succeed anyway). Expect 401 AuthMissingCertificate.
	commons_tests::server::run_with_tailnet_device_auth(
		"server",
		async |mut conn, _tailnet_ip, _node_id, device_id, _public, private| {
			provision_server(&mut conn, device_id).await;

			let response = private
				.post("/public/events")
				.add_header("Forwarded", "for=203.0.113.7")
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"message": "spoofed",
				}))
				.await;
			assert_eq!(response.status_code().as_u16(), 401);
			let body: serde_json::Value = response.json();
			assert_eq!(
				body.get("type").and_then(|v| v.as_str()),
				Some("/errors/auth-tailnet-identity-missing"),
			);
		},
	)
	.await
}
