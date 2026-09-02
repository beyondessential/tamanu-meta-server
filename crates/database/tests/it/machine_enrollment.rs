//! Model-level tests for application archival and the enrolment-token
//! lifecycle. A ticket admits a box, so it is minted against the machine.

use commons_types::server::{TagMap, app_type::ApplicationType};
use database::{
	Device, DeviceKey,
	applications::Application,
	machine_enrollment_tokens::MachineEnrollmentToken,
	machines::{Machine, NewMachine},
	pg_duration::PgDuration,
	url_field::UrlField,
};
use jiff::SignedDuration;
use uuid::Uuid;

fn new_server(host: &str, machine_id: Uuid) -> Application {
	Application {
		id: Uuid::new_v4(),
		name: Some("t".into()),
		host: Some(UrlField(host.parse().unwrap())),
		r#type: ApplicationType::TamanuCentral,
		rank: None,
		device_id: None,
		machine_id,
		group_id: None,
		public_name: None,
		cloud: None,
		geolocation: None,
		is_monitored: true,
		alert_when_down_for: PgDuration(SignedDuration::from_secs(600)),
		notes: String::new(),
		tags: TagMap::default(),
		deleted_at: None,
		registered_at: None,
		may_manage_dns: false,
		may_manage_tls: false,
		certificate_profile: None,
		name_management_paused_at: None,
		name_management_paused_by: None,
		name_management_pause_reason: None,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn soft_delete_releases_and_deactivates_device_and_hides_row() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		// Application bound to a device with an active key.
		let device = Device::create(&mut conn, b"key-bytes-1".to_vec())
			.await
			.unwrap();
		Device::trust(
			&mut conn,
			device.id,
			commons_types::device::DeviceRole::Machine,
		)
		.await
		.unwrap();
		let machine = Machine::create(&mut conn, NewMachine::default())
			.await
			.unwrap();
		let mut s = new_server("https://archive-me.example/", machine.id);
		s.device_id = Some(device.id);
		let server = Application::create(&mut conn, s).await.unwrap();

		Application::soft_delete(&mut conn, server.id)
			.await
			.unwrap();

		// Hidden from live listings, still resolvable by id.
		assert!(
			Application::get_all(&mut conn, 0, None)
				.await
				.unwrap()
				.iter()
				.all(|x| x.id != server.id),
			"archived server hidden from get_all"
		);
		let after = Application::get_by_id(&mut conn, server.id).await.unwrap();
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
		let replacement = Machine::create(&mut conn, NewMachine::default())
			.await
			.unwrap();
		Application::create(
			&mut conn,
			new_server("https://archive-me.example/", replacement.id),
		)
		.await
		.expect("host freed for reuse after archival");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn token_reissue_invalidates_prior_and_consume_is_single_use() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let machine = Machine::create(&mut conn, NewMachine::default())
			.await
			.unwrap();
		// No application on it: a ticket admits the box, and what runs on the
		// box only exists once the enrolled agent reports it.

		let (_t1, first) =
			MachineEnrollmentToken::mint(&mut conn, machine.id, SignedDuration::from_hours(1))
				.await
				.unwrap();
		// Reissue: the first token must no longer be active.
		let (t2, second) =
			MachineEnrollmentToken::mint(&mut conn, machine.id, SignedDuration::from_hours(1))
				.await
				.unwrap();
		assert!(
			MachineEnrollmentToken::find_active(&mut conn, machine.id, &first)
				.await
				.is_err(),
			"reissue invalidated the prior token"
		);
		assert!(
			MachineEnrollmentToken::find_active(&mut conn, machine.id, &second)
				.await
				.is_ok(),
			"reissued token is active"
		);

		// Consume is single-use.
		MachineEnrollmentToken::consume(&mut conn, machine.id, &t2.token_hash)
			.await
			.unwrap();
		assert!(
			MachineEnrollmentToken::consume(&mut conn, machine.id, &t2.token_hash)
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
		let machine = Machine::create(&mut conn, NewMachine::default())
			.await
			.unwrap();
		let (_t, token) =
			MachineEnrollmentToken::mint(&mut conn, machine.id, SignedDuration::from_hours(1))
				.await
				.unwrap();
		assert!(
			MachineEnrollmentToken::active_for(&mut conn, machine.id)
				.await
				.unwrap()
				.is_some()
		);

		MachineEnrollmentToken::revoke(&mut conn, machine.id)
			.await
			.unwrap();

		assert!(
			MachineEnrollmentToken::active_for(&mut conn, machine.id)
				.await
				.unwrap()
				.is_none(),
			"no active token after revoke"
		);
		assert!(
			MachineEnrollmentToken::find_active(&mut conn, machine.id, &token)
				.await
				.is_err(),
			"revoked token can't be presented"
		);
	})
	.await;
}
