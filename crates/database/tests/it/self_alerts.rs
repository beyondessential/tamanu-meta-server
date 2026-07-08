//! Self-alert lifecycle: a raise files one coalescing canopy-wide issue
//! which opens a canopy-wide incident, enqueuing exactly one Slack open on
//! the not-alerting → alerting transition (with flap grace below Critical,
//! immediate at Critical); recovery inside the grace cancels the open and
//! sends nothing; recovery after delivery enqueues the resolve; idle
//! recovers write nothing.

use commons_types::issue::Severity;
use database::self_alerts;
use database::slack_outbox::{KIND_INCIDENT_OPEN, KIND_INCIDENT_RESOLVE, SlackOutbox};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use jiff::Timestamp;

const REF: &str = "test-self-alert";

async fn outbox_rows(conn: &mut diesel_async::AsyncPgConnection) -> Vec<SlackOutbox> {
	use database::schema::slack_outbox::dsl;
	dsl::slack_outbox
		.select(SlackOutbox::as_select())
		.order(dsl::created_at.asc())
		.load(conn)
		.await
		.expect("load outbox")
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
		// by the grace (Error).
		let issue = self_alerts::raise(&mut conn, REF, Severity::Error, "title", "body")
			.await
			.expect("raise");
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
			"sub-Critical opens wait out the grace"
		);
		assert_eq!(open.payload["server"], "Canopy");
		assert_eq!(open.payload["source_ref"], format!("canopy/{REF}"));

		// Re-raise while alerting: no new outbox row.
		self_alerts::raise(&mut conn, REF, Severity::Error, "title", "body")
			.await
			.expect("re-raise");
		assert_eq!(outbox_rows(&mut conn).await.len(), 1);

		// Recover inside the grace: open cancelled, no resolve enqueued.
		self_alerts::recover(&mut conn, REF, "fixed")
			.await
			.expect("recover")
			.expect("was active");
		let rows = outbox_rows(&mut conn).await;
		assert_eq!(rows.len(), 1, "no resolve for a flap: {rows:?}");
		assert!(rows[0].gave_up_at.is_some(), "pending open cancelled");

		// Raise again: a fresh transition, a fresh open.
		self_alerts::raise(&mut conn, REF, Severity::Error, "title", "body")
			.await
			.expect("raise again");
		assert_eq!(outbox_rows(&mut conn).await.len(), 2);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn critical_ships_immediately_and_recovery_after_delivery_resolves() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		self_alerts::raise(&mut conn, REF, Severity::Critical, "title", "body")
			.await
			.expect("raise");
		let rows = outbox_rows(&mut conn).await;
		assert_eq!(rows.len(), 1);
		assert!(
			rows[0].deliver_after <= Timestamp::now(),
			"critical skips the grace"
		);

		// Simulate the drainer shipping the open, then recover: a resolve
		// row is enqueued and ships immediately.
		SlackOutbox::mark_delivered(&mut conn, rows[0].id, "ok")
			.await
			.expect("mark delivered");
		self_alerts::recover(&mut conn, REF, "identity ok")
			.await
			.expect("recover")
			.expect("was active");
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

		let issue = self_alerts::raise(&mut conn, REF, Severity::Critical, "title", "body")
			.await
			.expect("raise");
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
		self_alerts::raise(&mut conn, REF, Severity::Critical, "title", "body")
			.await
			.expect("re-raise");
		assert_eq!(outbox_rows(&mut conn).await.len(), 2);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn self_alert_issues_are_excluded_from_the_fleet_listing() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		use database::issues::Issue;

		self_alerts::raise(&mut conn, REF, Severity::Error, "title", "body")
			.await
			.expect("raise");

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
