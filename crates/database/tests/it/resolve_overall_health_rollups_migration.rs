//! The `2026-05-28-100000-0000_resolve_overall_health_rollups` migration
//! retires the `(status, health)` rollup issue category — see
//! `docs/plans/healthcheck-severity-catalog.md`. This test seeds the
//! pre-deprecation state (active rollup issue, incident contributed to
//! only by that issue, pending Slack `incident_open`) and replays the
//! migration's SQL to verify the full cleanup cascade.

use diesel::sql_types;
use diesel_async::{RunQueryDsl, SimpleAsyncConnection as _};
use uuid::Uuid;

const MIGRATION_UP: &str = include_str!(
	"../../../../migrations/2026-05-28-100000-0000_resolve_overall_health_rollups/up.sql"
);

#[tokio::test(flavor = "multi_thread")]
async fn migration_retires_active_rollup_and_orphan_incident() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group_id = Uuid::new_v4();
		let server_id = Uuid::new_v4();
		let device_id = Uuid::new_v4();
		let rollup_issue_id = Uuid::new_v4();
		let incident_id = Uuid::new_v4();
		let outbox_id = Uuid::new_v4();

		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role) VALUES ('{device_id}', 'server'); \
			 INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g'); \
			 INSERT INTO servers (id, host, kind, device_id, group_id) VALUES \
				('{server_id}', 'https://example.com', 'central', '{device_id}', '{group_id}'); \
			 INSERT INTO issues \
				(id, server_id, device_id, source, ref, severity, message, active, first_seen, last_seen) \
			   VALUES \
				('{rollup_issue_id}', '{server_id}', '{device_id}', 'status', 'health', 'error', \
				 'Server reports unhealthy', true, NOW() - interval '1 hour', NOW() - interval '5 min'); \
			 INSERT INTO incidents (id, server_group_id, opened_at) \
			   VALUES ('{incident_id}', '{group_id}', NOW() - interval '1 hour'); \
			 INSERT INTO incident_issues (incident_id, issue_id, joined_at) VALUES \
				('{incident_id}', '{rollup_issue_id}', NOW() - interval '1 hour'); \
			 INSERT INTO slack_outbox (id, kind, incident_id, issue_id, payload, deliver_after) \
			   VALUES \
				('{outbox_id}', 'incident_open', '{incident_id}', '{rollup_issue_id}', \
				 '{{}}'::jsonb, NOW() + interval '5 min');"
		))
		.await
		.expect("seed");

		// Replay the migration's up.sql.
		conn.batch_execute(MIGRATION_UP)
			.await
			.expect("replay migration up.sql");

		// 1. The rollup issue is deactivated and human-resolved.
		let row: IssueState = diesel::sql_query(
			"SELECT active, resolved_at IS NOT NULL AS resolved, resolved_by, resolved_reason \
			 FROM issues WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(rollup_issue_id)
		.get_result(&mut conn)
		.await
		.expect("rollup issue row still exists");
		assert!(!row.active, "rollup issue must be deactivated");
		assert!(row.resolved, "resolved_at must be set");
		assert_eq!(
			row.resolved_by.as_deref(),
			Some("migration:2026-05-28-resolve_overall_health_rollups")
		);
		assert_eq!(row.resolved_reason.as_deref(), Some("expected"));

		// 2. A close event with the synthetic hash + retirement message
		//    has been appended to the rollup issue.
		let evt: EventState = diesel::sql_query(
			"SELECT severity, active, message FROM events WHERE issue_id = $1 ORDER BY created_at DESC LIMIT 1",
		)
		.bind::<sql_types::Uuid, _>(rollup_issue_id)
		.get_result(&mut conn)
		.await
		.expect("close event was appended");
		assert_eq!(evt.severity, "info");
		assert!(!evt.active, "close event has active=false");
		assert_eq!(
			evt.message,
			"Overall-health roll-up retired; per-check issues remain."
		);

		// 3. The incident_issues link is marked left.
		let link: LinkState = diesel::sql_query(
			"SELECT left_at IS NOT NULL AS departed FROM incident_issues \
			 WHERE incident_id = $1 AND issue_id = $2",
		)
		.bind::<sql_types::Uuid, _>(incident_id)
		.bind::<sql_types::Uuid, _>(rollup_issue_id)
		.get_result(&mut conn)
		.await
		.expect("link row");
		assert!(
			link.departed,
			"incident_issues.left_at must be set after migration"
		);

		// 4. The orphan incident (no other live contributors) is closed.
		let inc: IncidentState = diesel::sql_query(
			"SELECT closed_at IS NOT NULL AS closed FROM incidents WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(incident_id)
		.get_result(&mut conn)
		.await
		.expect("incident row");
		assert!(inc.closed, "orphan incident must be closed");

		// 5. Pending Slack incident_open is cancelled (gave_up_at set,
		//    last_error explains why).
		let outbox: OutboxState = diesel::sql_query(
			"SELECT gave_up_at IS NOT NULL AS given_up, last_error \
			 FROM slack_outbox WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(outbox_id)
		.get_result(&mut conn)
		.await
		.expect("outbox row");
		assert!(outbox.given_up, "pending Slack open must be given up");
		assert!(
			outbox
				.last_error
				.as_deref()
				.is_some_and(|m| m.contains("overall-health rollup retirement migration")),
			"last_error explains the cancellation: {:?}",
			outbox.last_error
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn migration_leaves_incident_with_other_live_contributors_open() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group_id = Uuid::new_v4();
		let server_id = Uuid::new_v4();
		let device_id = Uuid::new_v4();
		let rollup_issue_id = Uuid::new_v4();
		let per_check_issue_id = Uuid::new_v4();
		let incident_id = Uuid::new_v4();

		// Rollup + a separate per-check issue both contributing to the
		// same incident. The per-check issue keeps the incident alive.
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role) VALUES ('{device_id}', 'server'); \
			 INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g'); \
			 INSERT INTO servers (id, host, kind, device_id, group_id) VALUES \
				('{server_id}', 'https://example.com', 'central', '{device_id}', '{group_id}'); \
			 INSERT INTO issues \
				(id, server_id, device_id, source, ref, severity, message, active, first_seen, last_seen) VALUES \
				('{rollup_issue_id}', '{server_id}', '{device_id}', 'status', 'health', \
				 'error', 'roll', true, NOW(), NOW()), \
				('{per_check_issue_id}', '{server_id}', '{device_id}', 'status', 'health/db', \
				 'error', 'db down', true, NOW(), NOW()); \
			 INSERT INTO incidents (id, server_group_id, opened_at) \
			   VALUES ('{incident_id}', '{group_id}', NOW()); \
			 INSERT INTO incident_issues (incident_id, issue_id, joined_at) VALUES \
				('{incident_id}', '{rollup_issue_id}', NOW()), \
				('{incident_id}', '{per_check_issue_id}', NOW());"
		))
		.await
		.expect("seed");

		conn.batch_execute(MIGRATION_UP)
			.await
			.expect("replay migration up.sql");

		let inc: IncidentState = diesel::sql_query(
			"SELECT closed_at IS NOT NULL AS closed FROM incidents WHERE id = $1",
		)
		.bind::<sql_types::Uuid, _>(incident_id)
		.get_result(&mut conn)
		.await
		.expect("incident row");
		assert!(
			!inc.closed,
			"incident must stay open while another live contributor exists"
		);
	})
	.await
}

#[derive(diesel::QueryableByName)]
struct IssueState {
	#[diesel(sql_type = sql_types::Bool)]
	active: bool,
	#[diesel(sql_type = sql_types::Bool)]
	resolved: bool,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	resolved_by: Option<String>,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	resolved_reason: Option<String>,
}

#[derive(diesel::QueryableByName)]
struct EventState {
	#[diesel(sql_type = sql_types::Text)]
	severity: String,
	#[diesel(sql_type = sql_types::Bool)]
	active: bool,
	#[diesel(sql_type = sql_types::Text)]
	message: String,
}

#[derive(diesel::QueryableByName)]
struct LinkState {
	#[diesel(sql_type = sql_types::Bool)]
	departed: bool,
}

#[derive(diesel::QueryableByName)]
struct IncidentState {
	#[diesel(sql_type = sql_types::Bool)]
	closed: bool,
}

#[derive(diesel::QueryableByName)]
struct OutboxState {
	#[diesel(sql_type = sql_types::Bool)]
	given_up: bool,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	last_error: Option<String>,
}
