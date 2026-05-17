use commons_types::{
	device::DeviceRole,
	server::{kind::ServerKind, rank::ServerRank, ticket::CanopyTicket},
};
use database::{devices::Device, servers::Server};
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
async fn upsert_from_ticket_persists_rank_and_trusts_device() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = Uuid::new_v4();
		let ticket = synthesize_ticket(id, "alpha", "https://alpha.example.com");
		let server = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Facility,
			Some(ServerRank::Production),
		)
		.await
		.expect("upsert");

		assert_eq!(server.rank, Some(ServerRank::Production));
		assert_eq!(server.kind, ServerKind::Facility);

		let device_id = server.device_id.expect("server has a device");
		let device = Device::get_with_info(&mut conn, device_id).await.expect("device").device;
		assert_eq!(device.role, DeviceRole::Server);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_from_ticket_re_import_refreshes_rank() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = Uuid::new_v4();
		let ticket = synthesize_ticket(id, "alpha", "https://alpha.example.com");

		let first = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Facility,
			Some(ServerRank::Demo),
		)
		.await
		.expect("first upsert");
		assert_eq!(first.rank, Some(ServerRank::Demo));

		let second = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Facility,
			Some(ServerRank::Production),
		)
		.await
		.expect("second upsert");
		assert_eq!(second.rank, Some(ServerRank::Production));
		assert_eq!(second.id, first.id);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_from_ticket_preserves_higher_role_on_existing_device() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = Uuid::new_v4();
		let ticket = synthesize_ticket(id, "alpha", "https://alpha.example.com");

		// First import → device gets Server role.
		let first = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Facility,
			Some(ServerRank::Production),
		)
		.await
		.expect("first upsert");
		let device_id = first.device_id.expect("device_id");

		// Manually promote to Admin (simulates an operator who chose
		// to give this device extra privileges).
		Device::trust(&mut conn, device_id, DeviceRole::Admin)
			.await
			.expect("trust admin");

		// Re-import the same ticket. The device should *not* be demoted.
		let _ = Server::upsert_from_ticket(
			&mut conn,
			&ticket,
			ServerKind::Facility,
			Some(ServerRank::Production),
		)
		.await
		.expect("second upsert");
		let device = Device::get_with_info(&mut conn, device_id).await.expect("device").device;
		assert_eq!(device.role, DeviceRole::Admin);
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
