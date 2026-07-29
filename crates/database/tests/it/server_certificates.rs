//! DB-layer tests for the CRT models: name registration and its exclusivity,
//! and the certificate order lifecycle — idempotent requests, retry backoff,
//! renewal, and the queries the alerts read.

use commons_tests::db::TestDb;
use database::diesel_async::AsyncPgConnection;
use database::server_certificates::{OrderState, RENEW_BEFORE};
use database::{ServerCertificate, ServerName};
use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use jiff::{SignedDuration, Timestamp};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(diesel::QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_server(conn: &mut AsyncPgConnection, name: &str) -> Uuid {
	let host = format!("https://{}.example.invalid", Uuid::new_v4());
	sql_query("INSERT INTO servers (name, host, kind) VALUES ($1, $2, 'central') RETURNING id")
		.bind::<sql_types::Text, _>(name)
		.bind::<sql_types::Text, _>(host)
		.get_result::<RowId>(conn)
		.await
		.expect("insert server")
		.id
}

fn addr(raw: &str) -> IpAddr {
	raw.parse().expect("address")
}

// ── Names ───────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn registering_normalises_and_starts_unpublished() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let row = ServerName::register(
			&mut conn,
			server,
			"Central.Fiji.Tamanu.App.",
			&[addr("192.0.2.1"), addr("2001:db8::1")],
		)
		.await
		.expect("register");

		assert_eq!(row.name, "central.fiji.tamanu.app");
		assert_eq!(row.wanted(), vec![addr("192.0.2.1"), addr("2001:db8::1")]);
		assert!(row.published().is_empty());
		assert!(!row.is_reconciled(), "nothing published yet");

		let work = ServerName::needing_publish(&mut conn, 10)
			.await
			.expect("work list");
		assert_eq!(work.len(), 1);
		assert_eq!(work[0].id, row.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn publishing_settles_the_registration() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let row = ServerName::register(&mut conn, server, "a.tamanu.app", &[addr("192.0.2.1")])
			.await
			.expect("register");

		ServerName::record_published(&mut conn, row.id, &[addr("192.0.2.1")])
			.await
			.expect("record published");

		let after = ServerName::for_name(&mut conn, "a.tamanu.app")
			.await
			.expect("read")
			.expect("present");
		assert!(after.is_reconciled());
		assert!(after.published_at.is_some());
		assert!(
			ServerName::needing_publish(&mut conn, 10)
				.await
				.expect("work list")
				.is_empty(),
			"a settled registration is not work"
		);

		// A change of address is work again.
		ServerName::register(&mut conn, server, "a.tamanu.app", &[addr("192.0.2.9")])
			.await
			.expect("re-register");
		assert_eq!(
			ServerName::needing_publish(&mut conn, 10)
				.await
				.expect("work list")
				.len(),
			1
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn address_order_is_not_a_change() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let row = ServerName::register(
			&mut conn,
			server,
			"a.tamanu.app",
			&[addr("192.0.2.1"), addr("192.0.2.2")],
		)
		.await
		.expect("register");
		// Published in the other order: the same set, so no work.
		ServerName::record_published(&mut conn, row.id, &[addr("192.0.2.2"), addr("192.0.2.1")])
			.await
			.expect("record published");
		assert!(
			ServerName::needing_publish(&mut conn, 10)
				.await
				.expect("work list")
				.is_empty()
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_name_belongs_to_one_server_at_a_time() {
	TestDb::run(async |mut conn, _url| {
		let one = insert_server(&mut conn, "one").await;
		let two = insert_server(&mut conn, "two").await;
		ServerName::register(&mut conn, one, "a.tamanu.app", &[addr("192.0.2.1")])
			.await
			.expect("first");

		let err = ServerName::register(&mut conn, two, "a.tamanu.app", &[addr("192.0.2.2")])
			.await
			.expect_err("the other server may not take it");
		assert!(
			matches!(err, commons_errors::AppError::Conflict(_)),
			"got {err:?}"
		);

		// The holder may keep changing its own.
		ServerName::register(&mut conn, one, "a.tamanu.app", &[addr("192.0.2.3")])
			.await
			.expect("holder updates");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn withdrawing_frees_the_name_once_records_are_gone() {
	TestDb::run(async |mut conn, _url| {
		let one = insert_server(&mut conn, "one").await;
		let two = insert_server(&mut conn, "two").await;
		let row = ServerName::register(&mut conn, one, "a.tamanu.app", &[addr("192.0.2.1")])
			.await
			.expect("register");
		ServerName::record_published(&mut conn, row.id, &[addr("192.0.2.1")])
			.await
			.expect("published");

		// No addresses: a withdrawal, which is work until the records are gone.
		let withdrawn = ServerName::register(&mut conn, one, "a.tamanu.app", &[])
			.await
			.expect("withdraw");
		assert!(withdrawn.is_withdrawing());
		assert!(!withdrawn.is_reconciled());

		// Still held until the reconcile has cleaned up.
		ServerName::register(&mut conn, two, "a.tamanu.app", &[addr("192.0.2.2")])
			.await
			.expect_err("not free yet");

		ServerName::record_published(&mut conn, row.id, &[])
			.await
			.expect("records removed");
		ServerName::forget(&mut conn, row.id).await.expect("forget");

		ServerName::register(&mut conn, two, "a.tamanu.app", &[addr("192.0.2.2")])
			.await
			.expect("free now");
	})
	.await;
}

// ── Certificates ────────────────────────────────────────────────────────────

const KEY_A: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
const KEY_B: &str = "bb00000000000000000000000000000000000000000000000000000000000000";

#[tokio::test(flavor = "multi_thread")]
async fn requesting_opens_one_order_per_name_and_key() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let first = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr-a")
			.await
			.expect("request");
		assert_eq!(first.order_state(), OrderState::Pending);
		assert!(!first.renewing, "a first issuance is not a renewal");

		// The same key again is the same order, not a second one.
		let again = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr-a")
			.await
			.expect("repeat");
		assert_eq!(again.id, first.id);

		// A different key for the same name is its own order.
		let other = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_B, b"csr-b")
			.await
			.expect("other key");
		assert_ne!(other.id, first.id);

		assert_eq!(
			ServerCertificate::for_server(&mut conn, server)
				.await
				.expect("list")
				.len(),
			2
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeat_request_is_answered_from_the_held_certificate() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let order = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr")
			.await
			.expect("request");
		let expiry = Timestamp::now() + SignedDuration::from_hours(90 * 24);
		ServerCertificate::record_issued(&mut conn, order.id, "-----BEGIN...", expiry)
			.await
			.expect("issued");

		let again = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr")
			.await
			.expect("repeat");
		assert_eq!(again.id, order.id);
		assert_eq!(again.order_state(), OrderState::Issued);
		assert!(again.is_current());
		assert_eq!(again.chain.as_deref(), Some("-----BEGIN..."));
		assert!(
			ServerCertificate::claim_due(&mut conn, 10)
				.await
				.expect("due")
				.is_empty(),
			"an issued certificate is no longer work"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_certificate_is_ordered_again_on_request() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let order = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr")
			.await
			.expect("request");
		let past = Timestamp::now() - SignedDuration::from_hours(1);
		ServerCertificate::record_issued(&mut conn, order.id, "chain", past)
			.await
			.expect("issued");

		let again = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr")
			.await
			.expect("repeat");
		assert_eq!(again.id, order.id);
		assert_eq!(again.order_state(), OrderState::Pending);
		assert!(again.renewing, "it had a chain, so this extends one");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_attempt_backs_off_and_stays_pending() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let order = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr")
			.await
			.expect("request");

		ServerCertificate::record_failure(&mut conn, order.id, "the authority said no")
			.await
			.expect("record failure");

		let after = ServerCertificate::get(&mut conn, order.id)
			.await
			.expect("get");
		assert_eq!(after.attempts, 1);
		assert_eq!(
			after.order_state(),
			OrderState::Pending,
			"still worth trying"
		);
		assert_eq!(after.last_error.as_deref(), Some("the authority said no"));
		assert!(after.next_attempt_at > Timestamp::now(), "backed off");
		assert!(
			ServerCertificate::claim_due(&mut conn, 10)
				.await
				.expect("due")
				.is_empty(),
			"not due until the backoff has passed"
		);

		// The second failure waits longer than the first.
		ServerCertificate::record_failure(&mut conn, order.id, "again")
			.await
			.expect("record failure");
		let second = ServerCertificate::get(&mut conn, order.id)
			.await
			.expect("get");
		assert_eq!(second.attempts, 2);
		assert!(second.next_attempt_at > after.next_attempt_at);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn renewal_picks_up_certificates_near_expiry_and_reuses_the_request() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;

		let soon =
			ServerCertificate::request(&mut conn, server, "soon.tamanu.app", KEY_A, b"csr-1")
				.await
				.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			soon.id,
			"chain",
			Timestamp::now() + RENEW_BEFORE - SignedDuration::from_hours(1),
		)
		.await
		.expect("issued");

		let later =
			ServerCertificate::request(&mut conn, server, "later.tamanu.app", KEY_B, b"csr-2")
				.await
				.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			later.id,
			"chain",
			Timestamp::now() + RENEW_BEFORE + SignedDuration::from_hours(48),
		)
		.await
		.expect("issued");

		let started = ServerCertificate::start_renewals(&mut conn)
			.await
			.expect("start renewals");
		assert_eq!(started.len(), 1, "only the one inside the window");
		assert_eq!(started[0].id, soon.id);
		assert!(started[0].renewing);
		assert_eq!(
			started[0].csr, b"csr-1",
			"renewal reuses the request the server sent, needing nothing from it"
		);
		assert!(
			started[0].chain.is_some(),
			"the old chain stays until the new one lands"
		);

		let due = ServerCertificate::claim_due(&mut conn, 10)
			.await
			.expect("due");
		assert_eq!(due.len(), 1);
		assert_eq!(due[0].id, soon.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_alert_queries_separate_expiring_from_never_issued() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;

		let expiring =
			ServerCertificate::request(&mut conn, server, "expiring.tamanu.app", KEY_A, b"csr-1")
				.await
				.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			expiring.id,
			"chain",
			Timestamp::now() + SignedDuration::from_hours(24),
		)
		.await
		.expect("issued");

		let never =
			ServerCertificate::request(&mut conn, server, "never.tamanu.app", KEY_B, b"csr-2")
				.await
				.expect("request");
		for _ in 0..4 {
			ServerCertificate::record_failure(&mut conn, never.id, "nope")
				.await
				.expect("fail");
		}

		let soon = ServerCertificate::expiring_within(&mut conn, SignedDuration::from_hours(48))
			.await
			.expect("expiring");
		assert_eq!(soon.len(), 1);
		assert_eq!(soon[0].id, expiring.id);

		let stuck = ServerCertificate::stuck_first_issuances(&mut conn, 3)
			.await
			.expect("stuck");
		assert_eq!(stuck.len(), 1, "only the one that never produced a chain");
		assert_eq!(stuck[0].id, never.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_an_order_takes_it_out_of_the_work_list() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let order = ServerCertificate::request(&mut conn, server, "a.tamanu.app", KEY_A, b"csr")
			.await
			.expect("request");

		ServerCertificate::stop(&mut conn, order.id, "the group released the domain")
			.await
			.expect("stop");

		assert!(
			ServerCertificate::claim_due(&mut conn, 10)
				.await
				.expect("due")
				.is_empty()
		);
		let after = ServerCertificate::get(&mut conn, order.id)
			.await
			.expect("get");
		assert_eq!(after.order_state(), OrderState::Failed);
		assert_eq!(
			after.last_error.as_deref(),
			Some("the group released the domain")
		);
	})
	.await;
}
