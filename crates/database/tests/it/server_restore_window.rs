//! Model-level tests for the per-server restore window.

use commons_types::server::{TagMap, kind::ServerKind};
use database::{pg_duration::PgDuration, servers::Server, url_field::UrlField};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

fn new_server() -> Server {
	Server {
		id: Uuid::new_v4(),
		name: Some("t".into()),
		host: Some(UrlField("https://restore.example/".parse().unwrap())),
		kind: ServerKind::Central,
		rank: None,
		device_id: None,
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
		restore_allowed_until: None,
		restore_allowed_by: None,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_window_opens_for_a_day_then_closes() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let created = Server::create(&mut conn, new_server()).await.unwrap();
		assert!(!created.restore_allowed(), "restores start disallowed");
		assert!(created.restore_allowed_until.is_none());

		// Opening the window returns an expiry roughly a day out.
		let until = Server::allow_restore(&mut conn, created.id, Some("op@example"))
			.await
			.unwrap();
		let secs = until.duration_since(Timestamp::now()).as_secs();
		assert!(
			(23 * 3600..=25 * 3600).contains(&secs),
			"window should be ~24h, got {secs}s"
		);

		let reloaded = Server::get_by_id(&mut conn, created.id).await.unwrap();
		assert!(reloaded.restore_allowed(), "window is open");
		// Postgres `timestamptz` keeps microseconds, so the round-tripped value
		// drops the nanosecond tail of the returned instant — compare with a
		// tolerance rather than for exact equality.
		let stored = reloaded.restore_allowed_until.expect("expiry persisted");
		assert!(
			stored.duration_since(until).as_secs().abs() <= 1,
			"stored expiry {stored} should match the returned {until}"
		);
		assert_eq!(reloaded.restore_allowed_by.as_deref(), Some("op@example"));

		// Closing the window clears both the expiry and the recorded operator.
		Server::disallow_restore(&mut conn, created.id)
			.await
			.unwrap();
		let reloaded = Server::get_by_id(&mut conn, created.id).await.unwrap();
		assert!(!reloaded.restore_allowed(), "window is closed");
		assert!(reloaded.restore_allowed_until.is_none());
		assert!(reloaded.restore_allowed_by.is_none());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_window_reads_as_closed() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let mut server = new_server();
		// A window that already lapsed: set but in the past.
		server.restore_allowed_until = Some(Timestamp::now() - SignedDuration::from_hours(1));
		server.restore_allowed_by = Some("op@example".into());
		let created = Server::create(&mut conn, server).await.unwrap();
		assert!(
			!created.restore_allowed(),
			"a past expiry must read as closed"
		);
	})
	.await;
}
