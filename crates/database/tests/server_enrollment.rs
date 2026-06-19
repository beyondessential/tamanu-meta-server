//! Model-level tests for server archival and enrollment-token lifecycle.

use commons_types::server::{TagMap, kind::ServerKind};
use database::{
	Device, DeviceKey, pg_duration::PgDuration, server_enrollment_tokens::ServerEnrollmentToken,
	servers::Server, url_field::UrlField,
};
use jiff::SignedDuration;
use uuid::Uuid;

fn new_server(host: &str) -> Server {
	Server {
		id: Uuid::new_v4(),
		name: Some("t".into()),
		host: Some(UrlField(host.parse().unwrap())),
		kind: ServerKind::Central,
		rank: None,
		device_id: None,
		group_id: None,
		public_name: None,
		cloud: None,
		geolocation: None,
		is_monitored: true,
		allow_legacy_status: false,
		alert_when_down_for: PgDuration(SignedDuration::from_secs(600)),
		notes: String::new(),
		tags: TagMap::default(),
		deleted_at: None,
		registered_at: None,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn soft_delete_releases_and_deactivates_device_and_hides_row() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		// Server bound to a device with an active key.
		let device = Device::create(&mut conn, b"key-bytes-1".to_vec())
			.await
			.unwrap();
		Device::trust(
			&mut conn,
			device.id,
			commons_types::device::DeviceRole::Server,
		)
		.await
		.unwrap();
		let mut s = new_server("https://archive-me.example/");
		s.device_id = Some(device.id);
		let server = Server::create(&mut conn, s).await.unwrap();

		Server::soft_delete(&mut conn, server.id).await.unwrap();

		// Hidden from live listings, still resolvable by id.
		assert!(
			Server::get_all(&mut conn, 0, None)
				.await
				.unwrap()
				.iter()
				.all(|x| x.id != server.id),
			"archived server hidden from get_all"
		);
		let after = Server::get_by_id(&mut conn, server.id).await.unwrap();
		assert!(after.deleted_at.is_some());
		assert!(after.device_id.is_none(), "device released");

		// Device demoted + key deactivated.
		let keys = DeviceKey::find_by_device(&mut conn, device.id)
			.await
			.unwrap();
		assert!(keys.is_empty(), "active keys gone (deactivated)");
		// from_key (active-only) no longer resolves the device.
		assert!(
			Device::from_key(&mut conn, b"key-bytes-1")
				.await
				.unwrap()
				.is_none()
		);

		// Recreating a server at the same host is allowed once archived.
		Server::create(&mut conn, new_server("https://archive-me.example/"))
			.await
			.expect("host freed for reuse after archival");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn token_reissue_invalidates_prior_and_consume_is_single_use() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let server = Server::create(&mut conn, new_server("https://tok.example/"))
			.await
			.unwrap();

		let (_t1, first) =
			ServerEnrollmentToken::mint(&mut conn, server.id, SignedDuration::from_hours(1))
				.await
				.unwrap();
		// Reissue: the first token must no longer be active.
		let (t2, second) =
			ServerEnrollmentToken::mint(&mut conn, server.id, SignedDuration::from_hours(1))
				.await
				.unwrap();
		assert!(
			ServerEnrollmentToken::find_active(&mut conn, server.id, &first)
				.await
				.is_err(),
			"reissue invalidated the prior token"
		);
		assert!(
			ServerEnrollmentToken::find_active(&mut conn, server.id, &second)
				.await
				.is_ok(),
			"reissued token is active"
		);

		// Consume is single-use.
		ServerEnrollmentToken::consume(&mut conn, server.id, &t2.token_hash)
			.await
			.unwrap();
		assert!(
			ServerEnrollmentToken::consume(&mut conn, server.id, &t2.token_hash)
				.await
				.is_err(),
			"second consume fails"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn revoke_invalidates_the_active_token() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let server = Server::create(&mut conn, new_server("https://revoke.example/"))
			.await
			.unwrap();
		let (_t, token) =
			ServerEnrollmentToken::mint(&mut conn, server.id, SignedDuration::from_hours(1))
				.await
				.unwrap();
		assert!(
			ServerEnrollmentToken::active_for(&mut conn, server.id)
				.await
				.unwrap()
				.is_some()
		);

		ServerEnrollmentToken::revoke(&mut conn, server.id)
			.await
			.unwrap();

		assert!(
			ServerEnrollmentToken::active_for(&mut conn, server.id)
				.await
				.unwrap()
				.is_none(),
			"no active token after revoke"
		);
		assert!(
			ServerEnrollmentToken::find_active(&mut conn, server.id, &token)
				.await
				.is_err(),
			"revoked token can't be presented"
		);
	})
	.await;
}
