//! DB-layer tests for the CRT models: name registration and its exclusivity,
//! and the certificate order lifecycle — idempotent requests, retry backoff,
//! renewal, and the queries the alerts read.

use commons_tests::db::TestDb;
use commons_types::dns::ManagedZone;
use database::diesel_async::AsyncPgConnection;
use database::server_certificates::{OrderState, RevocationReason, Risk, default_renew_after};
use database::{ServerCertificate, ServerGroupDomain, ServerName};
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
		ServerCertificate::record_issued(
			&mut conn,
			order.id,
			"-----BEGIN...",
			expiry,
			Some("classic"),
			None,
		)
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
		ServerCertificate::record_issued(&mut conn, order.id, "chain", past, None, None)
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
async fn renewal_follows_the_stored_window_and_reuses_the_request() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;

		// Due: the authority named a window that has already opened.
		let soon =
			ServerCertificate::request(&mut conn, server, "soon.tamanu.app", KEY_A, b"csr-1")
				.await
				.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			soon.id,
			"chain",
			Timestamp::now() + SignedDuration::from_hours(48),
			Some("shortlived"),
			Some(Timestamp::now() - SignedDuration::from_hours(1)),
		)
		.await
		.expect("issued");

		// Not due: a long-lived certificate issued just now.
		let later =
			ServerCertificate::request(&mut conn, server, "later.tamanu.app", KEY_B, b"csr-2")
				.await
				.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			later.id,
			"chain",
			Timestamp::now() + SignedDuration::from_hours(90 * 24),
			Some("classic"),
			None,
		)
		.await
		.expect("issued");

		let started = ServerCertificate::start_renewals(&mut conn)
			.await
			.expect("start renewals");
		assert_eq!(started.len(), 1, "only the one whose window has opened");
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
async fn the_renewal_window_scales_with_the_lifetime() {
	// A fixed window cannot serve both: it would leave a six-day certificate
	// permanently overdue, or renew a ninety-day one hundreds of times.
	let issued = Timestamp::now();

	let long = default_renew_after(issued, issued + SignedDuration::from_hours(90 * 24));
	let long_remaining: SignedDuration = (issued + SignedDuration::from_hours(90 * 24) - long)
		.try_into()
		.expect("duration");
	assert_eq!(long_remaining.as_hours(), 30 * 24, "a third of ninety days");

	let short = default_renew_after(issued, issued + SignedDuration::from_hours(6 * 24));
	let short_remaining: SignedDuration = (issued + SignedDuration::from_hours(6 * 24) - short)
		.try_into()
		.expect("duration");
	assert_eq!(short_remaining.as_hours(), 2 * 24, "a third of six days");

	// Already spent: renew at once rather than at some point in the past.
	assert_eq!(default_renew_after(issued, issued), issued);
	assert_eq!(
		default_renew_after(issued, issued - SignedDuration::from_hours(1)),
		issued
	);
}

/// A server with the TLS grant, in a group that controls `domain`.
async fn entitled_server(conn: &mut AsyncPgConnection, domain: &str) -> Uuid {
	let group = sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(format!("group-{}", Uuid::new_v4()))
		.get_result::<RowId>(conn)
		.await
		.expect("insert group")
		.id;
	let zones = ManagedZone::parse_list("tamanu.app=Z1", None).expect("zones");
	ServerGroupDomain::claim(conn, group, domain, None, &zones)
		.await
		.expect("claim domain");

	let host = format!("https://{}.example.invalid", Uuid::new_v4());
	sql_query(
		"INSERT INTO servers (name, host, kind, group_id, may_manage_tls) \
		 VALUES ($1, $2, 'central', $3, true) RETURNING id",
	)
	.bind::<sql_types::Text, _>("entitled")
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Uuid, _>(group)
	.get_result::<RowId>(conn)
	.await
	.expect("insert server")
	.id
}

#[tokio::test(flavor = "multi_thread")]
async fn risk_is_judged_against_the_certificates_own_lifetime() {
	TestDb::run(async |mut conn, _url| {
		let server = entitled_server(&mut conn, "fiji.tamanu.app").await;

		// Two days left. Comfortable on a ninety-day life, critical on a six-day
		// one — the same reading has to mean different things.
		let long =
			ServerCertificate::request(&mut conn, server, "long.fiji.tamanu.app", KEY_A, b"c1")
				.await
				.expect("request");
		sql_query(
			"UPDATE server_certificates SET state = 'issued', chain = 'x', \
			 issued_at = now() - interval '88 days', not_after = now() + interval '2 days' \
			 WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(long.id)
		.execute(&mut conn)
		.await
		.expect("age the long one");

		let short =
			ServerCertificate::request(&mut conn, server, "short.fiji.tamanu.app", KEY_B, b"c2")
				.await
				.expect("request");
		sql_query(
			"UPDATE server_certificates SET state = 'issued', chain = 'x', \
			 issued_at = now() - interval '4 days', not_after = now() + interval '2 days' \
			 WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(short.id)
		.execute(&mut conn)
		.await
		.expect("age the short one");

		let risks = ServerCertificate::at_risk(&mut conn)
			.await
			.expect("at risk");
		let by_id: std::collections::HashMap<Uuid, Risk> =
			risks.iter().map(|(cert, risk)| (cert.id, *risk)).collect();

		// 2 of 90 days left: well past even the critical fraction.
		assert_eq!(by_id.get(&long.id), Some(&Risk::Critical));
		// 2 of 6 days left: exactly the renewal point, so at risk but recoverable.
		assert_eq!(by_id.get(&short.id), Some(&Risk::AtRisk));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_certificate_for_an_unentitled_name_raises_nothing() {
	TestDb::run(async |mut conn, _url| {
		let server = entitled_server(&mut conn, "fiji.tamanu.app").await;
		let cert = ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect("request");
		sql_query(
			"UPDATE server_certificates SET state = 'issued', chain = 'x', \
			 issued_at = now() - interval '91 days', not_after = now() - interval '1 day' \
			 WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(cert.id)
		.execute(&mut conn)
		.await
		.expect("expire it");

		// Entitled and expired: reported.
		let risks = ServerCertificate::at_risk(&mut conn)
			.await
			.expect("at risk");
		assert_eq!(risks.len(), 1);
		assert_eq!(risks[0].1, Risk::Critical);

		// The group releases the domain. Canopy stopped renewing, so running out
		// is the intended outcome and there is nothing to report.
		sql_query("DELETE FROM server_group_domains")
			.execute(&mut conn)
			.await
			.expect("release the domain");
		assert!(
			ServerCertificate::at_risk(&mut conn)
				.await
				.expect("at risk")
				.is_empty(),
			"an unentitled name must leave no alert behind"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_the_grant_or_archiving_the_server_silences_the_alert() {
	TestDb::run(async |mut conn, _url| {
		let server = entitled_server(&mut conn, "fiji.tamanu.app").await;
		let cert = ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect("request");
		sql_query(
			"UPDATE server_certificates SET state = 'issued', chain = 'x', \
			 issued_at = now() - interval '91 days', not_after = now() - interval '1 day' \
			 WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(cert.id)
		.execute(&mut conn)
		.await
		.expect("expire it");
		assert_eq!(
			ServerCertificate::at_risk(&mut conn)
				.await
				.expect("at risk")
				.len(),
			1
		);

		sql_query("UPDATE servers SET may_manage_tls = false WHERE id = $1")
			.bind::<sql_types::Uuid, _>(server)
			.execute(&mut conn)
			.await
			.expect("revoke");
		assert!(
			ServerCertificate::at_risk(&mut conn)
				.await
				.expect("at risk")
				.is_empty(),
			"a revoked grant stops renewal, so expiry is expected"
		);

		// Granted again, and it is back in scope: entitlement is asked now, not
		// remembered from when renewal stopped.
		sql_query("UPDATE servers SET may_manage_tls = true WHERE id = $1")
			.bind::<sql_types::Uuid, _>(server)
			.execute(&mut conn)
			.await
			.expect("re-grant");
		assert_eq!(
			ServerCertificate::at_risk(&mut conn)
				.await
				.expect("at risk")
				.len(),
			1
		);

		sql_query("UPDATE servers SET deleted_at = now() WHERE id = $1")
			.bind::<sql_types::Uuid, _>(server)
			.execute(&mut conn)
			.await
			.expect("archive");
		assert!(
			ServerCertificate::at_risk(&mut conn)
				.await
				.expect("at risk")
				.is_empty(),
			"an archived server is not something to alert about"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_first_issuance_that_keeps_failing_is_told_apart() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, "central").await;
		let never =
			ServerCertificate::request(&mut conn, server, "never.tamanu.app", KEY_B, b"csr-2")
				.await
				.expect("request");
		for _ in 0..4 {
			ServerCertificate::record_failure(&mut conn, never.id, "nope")
				.await
				.expect("fail");
		}

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

#[tokio::test(flavor = "multi_thread")]
async fn revoking_stops_renewal_and_unholds_the_certificate() {
	TestDb::run(async |mut conn, _url| {
		let server = entitled_server(&mut conn, "fiji.tamanu.app").await;
		let cert = ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			cert.id,
			"chain",
			Timestamp::now() + SignedDuration::from_hours(48),
			Some("classic"),
			Some(Timestamp::now() - SignedDuration::from_hours(1)),
		)
		.await
		.expect("issued");
		// Due for renewal, until it isn't.
		assert_eq!(
			ServerCertificate::start_renewals(&mut conn)
				.await
				.expect("renewals")
				.len(),
			1
		);
		ServerCertificate::record_issued(
			&mut conn,
			cert.id,
			"chain",
			Timestamp::now() + SignedDuration::from_hours(48),
			Some("classic"),
			Some(Timestamp::now() - SignedDuration::from_hours(1)),
		)
		.await
		.expect("reissued");

		ServerCertificate::record_revoked(
			&mut conn,
			cert.id,
			RevocationReason::Superseded,
			Some("op@example.test"),
		)
		.await
		.expect("revoke");

		let after = ServerCertificate::get(&mut conn, cert.id)
			.await
			.expect("get");
		assert_eq!(after.order_state(), OrderState::Revoked);
		assert!(!after.is_current(), "a revoked certificate is not held");
		assert!(after.revoked_at.is_some());
		assert_eq!(after.revoked_by.as_deref(), Some("op@example.test"));
		assert_eq!(after.revocation_reason.as_deref(), Some("superseded"));
		assert!(after.renew_after.is_none(), "nothing left to renew");

		assert!(
			ServerCertificate::start_renewals(&mut conn)
				.await
				.expect("renewals")
				.is_empty(),
			"a revoked certificate is not renewed"
		);
		assert!(
			ServerCertificate::at_risk(&mut conn)
				.await
				.expect("at risk")
				.is_empty(),
			"a revoked certificate is not an expiry to chase"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_revoked_as_compromised_is_never_certified_again() {
	TestDb::run(async |mut conn, _url| {
		let server = entitled_server(&mut conn, "fiji.tamanu.app").await;
		let cert = ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			cert.id,
			"chain",
			Timestamp::now() + SignedDuration::from_hours(48),
			None,
			None,
		)
		.await
		.expect("issued");

		ServerCertificate::record_revoked(
			&mut conn,
			cert.id,
			RevocationReason::KeyCompromise,
			Some("op@example.test"),
		)
		.await
		.expect("revoke");

		// The same key, for the same name: refused.
		let err = ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect_err("a compromised key must not be certified again");
		assert!(
			matches!(err, commons_errors::AppError::BadRequest(_)),
			"got {err:?}"
		);

		// And for any other name, by anyone: a leaked key is leaked whoever asks.
		let other = insert_server(&mut conn, "other").await;
		ServerCertificate::request(&mut conn, other, "b.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect_err("still barred for another name and server");

		// A fresh key is fine.
		ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_B, b"c2")
			.await
			.expect("a new key is certifiable");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_for_another_reason_leaves_the_key_usable() {
	TestDb::run(async |mut conn, _url| {
		let server = entitled_server(&mut conn, "fiji.tamanu.app").await;
		let cert = ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect("request");
		ServerCertificate::record_issued(
			&mut conn,
			cert.id,
			"chain",
			Timestamp::now() + SignedDuration::from_hours(48),
			None,
			None,
		)
		.await
		.expect("issued");
		ServerCertificate::record_revoked(
			&mut conn,
			cert.id,
			RevocationReason::CessationOfOperation,
			None,
		)
		.await
		.expect("revoke");

		// A certificate retired says nothing about the key, so asking again
		// re-opens the order rather than being refused.
		let again = ServerCertificate::request(&mut conn, server, "a.fiji.tamanu.app", KEY_A, b"c")
			.await
			.expect("the key is still usable");
		assert_eq!(again.id, cert.id);
		assert_eq!(again.order_state(), OrderState::Pending);
	})
	.await;
}
