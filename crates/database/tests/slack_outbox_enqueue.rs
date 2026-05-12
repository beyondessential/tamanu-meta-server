//! Phase A: opening or closing an incident enqueues a `slack_outbox` row.
//!
//! Doesn't speak to Slack — the drainer is a separate binary and is tested
//! against a mock there. Here we just check the enqueue side: when an
//! incident transitions, an outbox row exists with the expected `kind` and
//! a non-empty `payload`.

use commons_types::issue::{ResolvedReason, Severity};
use database::{
	issues::{Incident, NewEvent},
	slack_outbox::{KIND_INCIDENT_OPEN, KIND_INCIDENT_RESOLVE, SlackOutbox},
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
	let row: RowId = sql_query(
		r#"
			INSERT INTO servers (host)
			VALUES ($1)
			RETURNING id
		"#,
	)
	.bind::<sql_types::Text, _>(host)
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
	use diesel::prelude::*;
	use database::schema::slack_outbox::dsl;
	dsl::slack_outbox
		.select(SlackOutbox::as_select())
		.filter(dsl::incident_id.eq(incident_id))
		.order(dsl::created_at.asc())
		.load(conn)
		.await
		.expect("load slack_outbox rows")
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
		// Payload is a flat object matching the workflow trigger's variables.
		let payload = open.payload.as_object().expect("payload is a JSON object");
		assert!(payload.contains_key("server"));
		assert_eq!(payload["severity"].as_str(), Some("Error"));
		assert_eq!(payload["source_ref"].as_str(), Some("test/ref-1"));
		assert_eq!(payload["message"].as_str(), Some("boom"));
		assert!(
			payload["link"]
				.as_str()
				.expect("link is a string")
				.contains("/incidents/"),
			"link looks like a canopy incident URL"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_incident_enqueues_resolve_row() {
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
		assert_eq!(opens, 1, "only the first issue opens; the second just joins");
	})
	.await
}
