//! Phase A: opening or closing an incident enqueues a `slack_outbox` row.
//!
//! Doesn't speak to Slack — the drainer is a separate binary and is tested
//! against a mock there. Here we just check the enqueue side: when an
//! incident transitions, an outbox row exists with the expected `kind` and
//! a non-empty `payload`.

use commons_types::issue::{ResolvedReason, Severity};
use database::{
	issues::{Incident, Issue, NewEvent},
	slack_outbox::{KIND_INCIDENT_OPEN, KIND_INCIDENT_RESOLVE, OPEN_DELAY, SlackOutbox},
};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_server(conn: &mut diesel_async::AsyncPgConnection, host: &str) -> Uuid {
	// Group + server pair so events can open incidents (incidents are
	// group-keyed; ungrouped servers don't promote issues to incidents).
	let group: RowId = sql_query(
		r#"
			INSERT INTO server_groups (name)
			VALUES ('test-group')
			RETURNING id
		"#,
	)
	.get_result(conn)
	.await
	.expect("insert group");
	let row: RowId = sql_query(
		r#"
			INSERT INTO servers (host, group_id)
			VALUES ($1, $2)
			RETURNING id
		"#,
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Uuid, _>(group.id)
	.get_result(conn)
	.await
	.expect("insert server");
	row.id
}

async fn pending_for_incident(
	conn: &mut diesel_async::AsyncPgConnection,
	incident_id: Uuid,
) -> Vec<SlackOutbox> {
	use database::diesel_async::RunQueryDsl;
	use database::schema::slack_outbox::dsl;
	use diesel::prelude::*;
	dsl::slack_outbox
		.select(SlackOutbox::as_select())
		.filter(dsl::incident_id.eq(incident_id))
		.order(dsl::created_at.asc())
		.load(conn)
		.await
		.expect("load slack_outbox rows")
}

/// Force-deliver the open row for `incident_id` so a subsequent resolve
/// no longer treats it as cancellable. Used by tests that want to
/// exercise the post-delivery code path (where the resolve actually does
/// enqueue) without waiting `OPEN_DELAY` for the real drainer.
async fn mark_open_delivered(conn: &mut diesel_async::AsyncPgConnection, incident_id: Uuid) {
	use database::diesel_async::RunQueryDsl;
	use database::schema::slack_outbox::dsl;
	use diesel::prelude::*;
	let row_id: Uuid = dsl::slack_outbox
		.select(dsl::id)
		.filter(dsl::incident_id.eq(incident_id))
		.filter(dsl::kind.eq(KIND_INCIDENT_OPEN))
		.first(conn)
		.await
		.expect("open row exists");
	SlackOutbox::mark_delivered(conn, row_id, "ok")
		.await
		.expect("mark delivered");
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_incident_enqueues_slack_open_row() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, "http://open.invalid/").await;
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-1".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "boom".into(),
			active: Some(true),
			occurred_at: None,
		};
		let issue = event.save(&mut conn, server_id, None).await.expect("save");

		// The save call should have opened an incident *and* enqueued an
		// `incident_open` outbox row.
		let incident: Incident = Incident::list_for_server(&mut conn, server_id, false, 10)
			.await
			.expect("list incidents")
			.into_iter()
			.next()
			.expect("issue opened an incident");
		let _ = issue.id;

		let rows = pending_for_incident(&mut conn, incident.id).await;
		let opens: Vec<_> = rows
			.iter()
			.filter(|r| r.kind == KIND_INCIDENT_OPEN)
			.collect();
		assert_eq!(opens.len(), 1, "exactly one open row");
		let open = opens[0];
		assert_eq!(open.issue_id, Some(issue.id));
		assert!(open.delivered_at.is_none());
		assert_eq!(open.attempts, 0);
		// Open rows wait OPEN_DELAY (3 minutes today) before the drainer
		// is allowed to ship them. `created_at` is set server-side by
		// the migration default, so the gap should equal OPEN_DELAY
		// give-or-take the round-trip time of the enqueue itself.
		let target = open.created_at + OPEN_DELAY;
		let drift = (open.deliver_after - target).get_seconds().unsigned_abs();
		assert!(
			drift <= 5,
			"deliver_after should sit ~OPEN_DELAY past created_at; drift={drift}s \
			 (created_at={}, deliver_after={})",
			open.created_at,
			open.deliver_after,
		);
		// Payload is a flat object matching the workflow trigger's variables.
		// `link` is intentionally absent here — the drainer injects it at
		// delivery time from PRIVATE_URL + row.incident_id.
		let payload = open.payload.as_object().expect("payload is a JSON object");
		assert!(payload.contains_key("server"));
		assert_eq!(payload["severity"].as_str(), Some("Error"));
		assert_eq!(payload["source_ref"].as_str(), Some("test/ref-1"));
		assert_eq!(payload["message"].as_str(), Some("boom"));
		assert!(
			!payload.contains_key("link"),
			"link is injected by the drainer, not at enqueue"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_incident_after_open_delivered_enqueues_resolve_row() {
	// Once Slack has heard about the open, we owe a resolve. The
	// flap-suppression path only kicks in when the open hasn't shipped
	// yet; everything past that point is the historical behaviour.
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, "http://resolve.invalid/").await;
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-2".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "boom".into(),
			active: Some(true),
			occurred_at: None,
		};
		event.save(&mut conn, server_id, None).await.expect("save");
		let incident = Incident::list_for_server(&mut conn, server_id, false, 10)
			.await
			.expect("list incidents")
			.into_iter()
			.next()
			.expect("incident opened");
		mark_open_delivered(&mut conn, incident.id).await;

		Incident::resolve(
			&mut conn,
			incident.id,
			"operator@example.test",
			ResolvedReason::Fixed,
		)
		.await
		.expect("resolve");

		let rows = pending_for_incident(&mut conn, incident.id).await;
		let resolves: Vec<_> = rows
			.iter()
			.filter(|r| r.kind == KIND_INCIDENT_RESOLVE)
			.collect();
		assert_eq!(resolves.len(), 1, "exactly one resolve row");
		assert!(resolves[0].delivered_at.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_before_open_ships_cancels_open_and_skips_resolve() {
	// The flap-suppression contract. If the incident comes and goes
	// inside the `deliver_after` window, the operator never sees a Slack
	// noise about either edge — but the open row stays in the database
	// (given-up, with a reason in `last_error`) for the audit trail.
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, "http://flap.invalid/").await;
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-flap".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "boom".into(),
			active: Some(true),
			occurred_at: None,
		};
		event.save(&mut conn, server_id, None).await.expect("save");
		let incident = Incident::list_for_server(&mut conn, server_id, false, 10)
			.await
			.expect("list incidents")
			.into_iter()
			.next()
			.expect("incident opened");

		// Resolve before the open's deliver_after window passes.
		Incident::resolve(
			&mut conn,
			incident.id,
			"operator@example.test",
			ResolvedReason::Fixed,
		)
		.await
		.expect("resolve");

		let rows = pending_for_incident(&mut conn, incident.id).await;
		let opens: Vec<_> = rows
			.iter()
			.filter(|r| r.kind == KIND_INCIDENT_OPEN)
			.collect();
		assert_eq!(opens.len(), 1, "open row stays in the table for historicity");
		assert!(
			opens[0].gave_up_at.is_some(),
			"open row marked given-up so the drainer won't ship it"
		);
		assert!(opens[0].delivered_at.is_none(), "open never delivered");
		assert!(
			opens[0]
				.last_error
				.as_deref()
				.is_some_and(|e| e.contains("cancelled")),
			"reason is recorded for the audit trail; got: {:?}",
			opens[0].last_error
		);

		let resolves: Vec<_> = rows
			.iter()
			.filter(|r| r.kind == KIND_INCIDENT_RESOLVE)
			.collect();
		assert!(
			resolves.is_empty(),
			"no resolve row when the matching open never went to Slack"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn cascade_close_via_issue_resolve_attributes_to_operator() {
	// When the operator resolves the last live issue and the cascade
	// closes the incident, the resulting Slack resolve row must credit
	// the operator — not say "automation". Earlier behavior threaded
	// `None` through the cascade path and lost the attribution.
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, "http://attribute.invalid/").await;
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-only".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "boom".into(),
			active: Some(true),
			occurred_at: None,
		};
		let issue = event.save(&mut conn, server_id, None).await.expect("save");
		let incident = Incident::list_for_server(&mut conn, server_id, false, 10)
			.await
			.expect("list incidents")
			.into_iter()
			.next()
			.expect("incident opened");
		// Pretend the drainer already shipped the open so the cascade
		// resolve actually enqueues — otherwise the open would be
		// cancelled and there'd be no resolve row to inspect.
		mark_open_delivered(&mut conn, incident.id).await;

		Issue::resolve(
			&mut conn,
			issue.id,
			"operator@example.test",
			ResolvedReason::Fixed,
		)
		.await
		.expect("resolve issue");

		let rows = pending_for_incident(&mut conn, incident.id).await;
		let resolve = rows
			.iter()
			.find(|r| r.kind == KIND_INCIDENT_RESOLVE)
			.expect("resolve row enqueued");
		assert_eq!(
			resolve.payload["by"].as_str(),
			Some("operator@example.test"),
			"cascade close must credit the operator, not automation"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn nil_server_events_do_not_open_incidents() {
	// The meta/nil server hosts canopy's own self-monitoring events
	// (e.g. "Slack delivery failure"). It is intentionally **ungrouped**
	// in the new model, so events file as issues but never roll up to a
	// group-level incident — which means the Slack drainer can never
	// loop back into itself by re-firing on a canopy-self failure.
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let event = NewEvent {
			source: "canopy".into(),
			r#ref: "slack-delivery-failure".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "boom".into(),
			active: Some(true),
			occurred_at: None,
		};
		event
			.save(&mut conn, Uuid::nil(), None)
			.await
			.expect("save");

		let incidents = Incident::list_for_server(&mut conn, Uuid::nil(), false, 10)
			.await
			.expect("list incidents");
		assert!(
			incidents.is_empty(),
			"nil/meta server is ungrouped — events don't open incidents",
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn mark_given_up_removes_row_from_claim_pending() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, "http://giveup.invalid/").await;
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-g".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "boom".into(),
			active: Some(true),
			occurred_at: None,
		};
		event.save(&mut conn, server_id, None).await.expect("save");
		// `event.save` enqueues an open whose deliver_after sits
		// OPEN_DELAY in the future. Drop it back to the past so
		// claim_pending will return the row immediately.
		expire_deliver_after(&mut conn).await;
		let row = SlackOutbox::claim_pending(&mut conn, 10)
			.await
			.expect("claim")
			.into_iter()
			.next()
			.expect("one pending row");

		SlackOutbox::mark_given_up(&mut conn, row.id, "deliberately abandoned")
			.await
			.expect("mark_given_up");

		let still_pending = SlackOutbox::claim_pending(&mut conn, 10)
			.await
			.expect("claim again");
		assert!(
			still_pending.iter().all(|r| r.id != row.id),
			"gave-up row must not be reclaimed"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn claim_pending_skips_rows_whose_deliver_after_is_in_the_future() {
	// Direct check on the drainer's claim filter: an open row that's
	// still inside its OPEN_DELAY window must not be claimed, but the
	// moment its deliver_after slides into the past it becomes
	// claimable. This is the core flap-suppression mechanism.
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, "http://delayed.invalid/").await;
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-d".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "boom".into(),
			active: Some(true),
			occurred_at: None,
		};
		event.save(&mut conn, server_id, None).await.expect("save");
		let pending_before = SlackOutbox::claim_pending(&mut conn, 10)
			.await
			.expect("claim");
		assert!(
			pending_before.is_empty(),
			"open row must wait OPEN_DELAY before becoming claimable; got {} rows",
			pending_before.len(),
		);

		expire_deliver_after(&mut conn).await;

		let pending_after = SlackOutbox::claim_pending(&mut conn, 10)
			.await
			.expect("claim again");
		assert_eq!(
			pending_after.len(),
			1,
			"row becomes claimable once deliver_after is in the past",
		);
	})
	.await
}

/// Backdate every pending row's `deliver_after` so the drainer can pick
/// them up without the test having to sleep through `OPEN_DELAY`.
async fn expire_deliver_after(conn: &mut diesel_async::AsyncPgConnection) {
	sql_query("UPDATE slack_outbox SET deliver_after = NOW() - INTERVAL '1 minute' WHERE delivered_at IS NULL AND gave_up_at IS NULL")
		.execute(conn)
		.await
		.expect("backdate deliver_after");
}

#[tokio::test(flavor = "multi_thread")]
async fn rejoining_open_incident_does_not_re_enqueue_open() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, "http://rejoin.invalid/").await;
		let event_a = NewEvent {
			source: "test".into(),
			r#ref: "ref-a".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "first".into(),
			active: Some(true),
			occurred_at: None,
		};
		event_a
			.save(&mut conn, server_id, None)
			.await
			.expect("save a");
		let incident = Incident::list_for_server(&mut conn, server_id, false, 10)
			.await
			.expect("list incidents")
			.into_iter()
			.next()
			.expect("incident");

		// Second active issue, same server: joins the existing incident.
		let event_b = NewEvent {
			source: "test".into(),
			r#ref: "ref-b".into(),
			severity: Some(Severity::Error),
			description: None,
			message: "second".into(),
			active: Some(true),
			occurred_at: None,
		};
		event_b
			.save(&mut conn, server_id, None)
			.await
			.expect("save b");

		let rows = pending_for_incident(&mut conn, incident.id).await;
		let opens = rows.iter().filter(|r| r.kind == KIND_INCIDENT_OPEN).count();
		assert_eq!(
			opens, 1,
			"only the first issue opens; the second just joins"
		);
	})
	.await
}
