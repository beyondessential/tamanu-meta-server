//! Integration test for the `/api/servers/import_ticket` endpoint as
//! it interacts with tailnet first-contact placeholders. The expected
//! flow is: a server first comes up over the tailnet, canopy auto-
//! creates an Untrusted device row holding its node id; the operator
//! later imports the server's ticket, which carries the same
//! tailscale identifier. The placeholder should be detached and the
//! ticket's freshly-created device should adopt the identity.

use axum_client_ip::ClientIpSource;
use commons_servers::{
	router,
	tailnet_directory::{DirectoryEntry, TailnetDirectory},
};
use commons_tests::axum_test::TestServer;
use commons_tests::db::TestDb;
use commons_tests::diesel_async::SimpleAsyncConnection;
use commons_types::server::{kind::ServerKind, rank::ServerRank, ticket::CanopyTicket};
use uuid::Uuid;

fn test_directory() -> (std::net::IpAddr, String, TailnetDirectory) {
	let ip: std::net::IpAddr = "100.64.0.7".parse().unwrap();
	let node_id = "nodekey:importtest7".to_string();
	let dir = TailnetDirectory::for_test([(
		ip,
		DirectoryEntry {
			node_id: node_id.clone(),
			node_name: "import-test.example.ts.net".into(),
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
	let router = router(
		private_server::routes(private_server::state::AppState {
			db: database::init_to(url),
			ro_pool: None,
			tailnet_directory: Some(directory),
		})
		.unwrap(),
		ClientIpSource::RightmostForwarded,
	);
	let mut server = TestServer::new(router);
	server.add_header("Forwarded", "for=192.0.2.10");
	server
}

fn synthesize_ticket(
	server_id: Uuid,
	hostname: &str,
	url: &str,
	tailscale_ip: &str,
) -> CanopyTicket {
	use rcgen::{KeyPair, PKCS_ECDSA_P256_SHA256};

	let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("keygen");
	let pem = key.public_key_pem();
	CanopyTicket {
		v: "ticket-1".into(),
		server_id,
		public_key: pem,
		hostname: hostname.into(),
		tailscale_ip: Some(tailscale_ip.into()),
		tailscale_name: Some(format!("{hostname}.example.ts.net")),
		canonical_url: url.into(),
		hosting: None,
		kind: None,
		rank: None,
	}
}

fn encode_ticket(ticket: &CanopyTicket) -> String {
	use base64::Engine as _;
	let json = serde_json::to_vec(ticket).expect("ticket json");
	base64::prelude::BASE64_STANDARD.encode(&json)
}

#[tokio::test(flavor = "multi_thread")]
async fn import_ticket_detaches_untrusted_tailnet_placeholder() {
	TestDb::run(async |mut conn, url| {
		let (ip, node_id, dir) = test_directory();
		let private = private_with_directory(&url, dir).await;

		// Tailnet first-contact has already auto-created an Untrusted
		// placeholder for this node id.
		let placeholder = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role, tailscale_node_id, tailscale_node_name) \
			 VALUES ('{placeholder}', 'untrusted', '{node_id}', 'placeholder-name');"
		))
		.await
		.expect("seed placeholder");

		let server_id = Uuid::new_v4();
		let ticket = synthesize_ticket(
			server_id,
			"import-test",
			"https://import-test.example.com",
			&ip.to_string(),
		);
		let resp = private
			.post("/api/servers/import_ticket")
			.json(&serde_json::json!({
				"ticket_b64": encode_ticket(&ticket),
				"kind": ServerKind::Facility,
				"rank": Option::<ServerRank>::None,
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body.as_str(), Some(server_id.to_string().as_str()));

		// Ticket's freshly-created device should now hold the identity,
		// and the placeholder's tailscale_* columns should be cleared.
		// Both rows still exist — detach, not discard.
		conn.batch_execute(&format!(
			"DO $$ BEGIN \
			   IF NOT EXISTS ( \
			     SELECT 1 FROM servers s \
			     JOIN devices d ON s.device_id = d.id \
			     WHERE s.id = '{server_id}' AND d.tailscale_node_id = '{node_id}' \
			   ) THEN RAISE EXCEPTION 'ticket device should hold the identity'; END IF; \
			   IF NOT EXISTS ( \
			     SELECT 1 FROM devices \
			     WHERE id = '{placeholder}' AND tailscale_node_id IS NULL \
			   ) THEN RAISE EXCEPTION 'placeholder should be detached'; END IF; \
			 END $$;"
		))
		.await
		.expect("verify rows");
	})
	.await
}
