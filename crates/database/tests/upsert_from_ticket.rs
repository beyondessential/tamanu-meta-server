use commons_types::{
	server::{kind::ServerKind, rank::ServerRank, ticket::CanopyTicket},
};
use database::servers::Server;
use uuid::Uuid;

fn synthesize_ticket(server_id: Uuid, hostname: &str, url: &str) -> CanopyTicket {
	use rcgen::{KeyPair, PKCS_ECDSA_P256_SHA256};

	let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("keygen");
	let pem = key.public_key_pem();
	CanopyTicket {
		v: "ticket-1".into(),
		server_id,
		public_key: pem,
		hostname: hostname.into(),
		tailscale_ip: Some("100.64.0.42".into()),
		tailscale_name: Some(format!("{hostname}.example.ts.net")),
		canonical_url: url.into(),
		hosting: None,
		kind: None,
		rank: None,
		central_public_key: None,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_from_ticket_smoke() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = Uuid::new_v4();
		let ticket = synthesize_ticket(id, "alpha", "https://alpha.example.com");
		let server = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Central,
			Some(ServerRank::Production),
		)
		.await
		.expect("upsert");
		assert_eq!(server.id, id);
		assert_eq!(server.name.as_deref(), Some("alpha"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_from_ticket_idempotent() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = Uuid::new_v4();
		let ticket = synthesize_ticket(id, "alpha", "https://alpha.example.com");
		let _a = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Central,
			Some(ServerRank::Production),
		)
		.await
		.expect("first upsert");
		let _b = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Central,
			Some(ServerRank::Production),
		)
		.await
		.expect("second upsert");
	})
	.await
}
