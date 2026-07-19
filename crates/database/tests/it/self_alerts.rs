//! Self-alert lifecycle: a raise files one coalescing canopy-wide issue
//! which opens a canopy-wide incident, enqueuing exactly one Slack open on
//! the not-alerting → alerting transition (with flap grace for
//! non-escalating checks, immediate for escalating ones); recovery starts
//! the incident lingering, and a re-raise within the linger continues the
//! same incident without a new notification; a flap whose open never
//! shipped is cancelled silently at linger expiry; recovery after delivery
//! enqueues the resolve at linger expiry; idle recovers write nothing.

use commons_types::status::CheckResult;
use database::self_alerts;
use database::slack_outbox::{KIND_INCIDENT_OPEN, KIND_INCIDENT_RESOLVE, SlackOutbox};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use jiff::Timestamp;

const REF: &str = "test-self-alert";

/// Raise REF as a failure; `escalates` picks the registered policy tier
/// (escalating ≙ the old Critical, immediate notify; otherwise graced).
/// Each test runs on a fresh database, so the first raise registers the
/// catalog entry at the given tier.
async fn raise_with(
	conn: &mut diesel_async::AsyncPgConnection,
	escalates: bool,
) -> database::issues::Issue {
	self_alerts::raise(
		conn,
		REF,
		CheckResult::Failed,
		CheckResult::Failed,
		escalates,
		None,
		"title",
		"body",
	)
	.await
	.expect("raise")
}

async fn outbox_rows(conn: &mut diesel_async::AsyncPgConnection) -> Vec<SlackOutbox> {
	use database::schema::slack_outbox::dsl;
	dsl::slack_outbox
		.select(SlackOutbox::as_select())
		.order(dsl::created_at.asc())
		.load(conn)
		.await
		.expect("load outbox")
}

/// Backdate the canopy-wide incident's `closing_at` past the global linger
/// window, then run the sweep — the test-speed way to let the close-side
/// grace elapse.
async fn expire_linger(conn: &mut diesel_async::AsyncPgConnection) {
	diesel::sql_query(
		"UPDATE incidents SET closing_at = closing_at - INTERVAL '1 hour' \
		 WHERE server_group_id IS NULL",
	)
	.execute(conn)
	.await
	.expect("expire linger");
	database::issues::sweep_lingering_incidents(conn)
		.await
		.expect("linger sweep");
}

#[tokio::test(flavor = "multi_thread")]
async fn raise_enqueues_once_and_flap_recovery_is_silent() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		// Idle recover: nothing exists, nothing written.
		assert!(
			self_alerts::recover(&mut conn, REF, "nothing to do")
				.await
				.expect("recover")
				.is_none()
		);
		assert!(outbox_rows(&mut conn).await.is_empty());

		// First raise: issue + canopy-wide incident + one open row, delayed
		// by the grace (non-escalating).
		let issue = raise_with(&mut conn, false).await;
		assert_eq!(issue.server_id, None);
		assert_eq!(issue.server_group_id, None);
		assert!(issue.active);
		let rows = outbox_rows(&mut conn).await;
		let [open] = rows.as_slice() else {
			panic!("exactly one outbox row, got {rows:?}");
		};
		assert_eq!(open.kind, KIND_INCIDENT_OPEN);
		assert!(
			open.incident_id.is_some(),
			"the raise opened a canopy-wide incident"
		);
		assert_eq!(open.issue_id, Some(issue.id));
		assert!(
			open.deliver_after > Timestamp::now(),
			"non-escalating opens wait out the grace"
		);
		assert_eq!(open.payload["server"], "Canopy");
		assert_eq!(open.payload["source_ref"], format!("canopy/{REF}"));

		// Re-raise while alerting: no new outbox row.
		raise_with(&mut conn, false).await;
		assert_eq!(outbox_rows(&mut conn).await.len(), 1);

		// Recover: the incident lingers — the pending open is neither
		// shipped nor cancelled yet, and no resolve is enqueued.
		self_alerts::recover(&mut conn, REF, "fixed")
			.await
			.expect("recover")
			.expect("was active");
		let rows = outbox_rows(&mut conn).await;
		assert_eq!(rows.len(), 1, "no resolve while lingering: {rows:?}");
		assert!(
			rows[0].gave_up_at.is_none(),
			"pending open survives the linger"
		);

		// Re-raise within the linger: the same incident continues — no
		// second open row. This is the close-then-reopen noise the linger
		// exists to absorb.
		raise_with(&mut conn, false).await;
		let rows = outbox_rows(&mut conn).await;
		assert_eq!(rows.len(), 1, "rejoin continues the incident: {rows:?}");

		// Recover again and let the linger elapse: the open (never shipped)
		// is cancelled and no resolve follows — the whole flap was silent.
		self_alerts::recover(&mut conn, REF, "fixed again")
			.await
			.expect("recover")
			.expect("was active");
		expire_linger(&mut conn).await;
		let rows = outbox_rows(&mut conn).await;
		assert_eq!(rows.len(), 1, "no resolve for a flap: {rows:?}");
		assert!(rows[0].gave_up_at.is_some(), "pending open cancelled");

		// Raise again: a fresh transition, a fresh open.
		raise_with(&mut conn, false).await;
		assert_eq!(outbox_rows(&mut conn).await.len(), 2);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn escalating_ships_immediately_and_recovery_after_delivery_resolves() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		raise_with(&mut conn, true).await;
		let rows = outbox_rows(&mut conn).await;
		assert_eq!(rows.len(), 1);
		assert!(
			rows[0].deliver_after <= Timestamp::now(),
			"an escalating check skips the grace"
		);

		// Simulate the drainer shipping the open, then recover: the incident
		// lingers first, and the resolve is enqueued when the linger elapses.
		SlackOutbox::mark_delivered(&mut conn, rows[0].id, "ok")
			.await
			.expect("mark delivered");
		self_alerts::recover(&mut conn, REF, "identity ok")
			.await
			.expect("recover")
			.expect("was active");
		assert_eq!(
			outbox_rows(&mut conn).await.len(),
			1,
			"resolve waits out the linger"
		);
		expire_linger(&mut conn).await;
		let rows = outbox_rows(&mut conn).await;
		assert_eq!(rows.len(), 2);
		let resolve = &rows[1];
		assert_eq!(resolve.kind, KIND_INCIDENT_RESOLVE);
		assert_eq!(resolve.payload["server"], "Canopy");
		assert!(resolve.deliver_after <= Timestamp::now());

		// Recovering again is a no-op.
		assert!(
			self_alerts::recover(&mut conn, REF, "still ok")
				.await
				.expect("recover")
				.is_none()
		);
		assert_eq!(outbox_rows(&mut conn).await.len(), 2);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn operator_resolved_but_persisting_condition_re_notifies() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		use database::issues::Issue;

		let issue = raise_with(&mut conn, true).await;
		assert_eq!(outbox_rows(&mut conn).await.len(), 1);

		// An operator resolves it, but the condition still holds: the next
		// raise clears the resolution and notifies again — the operator's
		// claim of "fixed" was wrong and silence would hide that.
		Issue::resolve(
			&mut conn,
			issue.id,
			"op@example.com",
			commons_types::issue::ResolvedReason::Fixed,
		)
		.await
		.expect("operator resolve");
		raise_with(&mut conn, true).await;
		assert_eq!(outbox_rows(&mut conn).await.len(), 2);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn self_alert_issues_are_excluded_from_the_fleet_listing() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		use database::issues::Issue;

		raise_with(&mut conn, false).await;

		let fleet = Issue::list(&mut conn, Default::default(), 100)
			.await
			.expect("list");
		assert!(
			fleet.is_empty(),
			"canopy-wide issues must not appear in the fleet listing: {fleet:?}"
		);
		let alerts = self_alerts::list(&mut conn, 50).await.expect("alerts");
		assert_eq!(alerts.len(), 1);
	})
	.await
}
