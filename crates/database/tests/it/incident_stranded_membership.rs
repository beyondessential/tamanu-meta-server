//! Membership rows stranded in *closed* incidents must not gate future
//! incidents.
//!
//! Closing an incident retires it without stamping `left_at` on the members
//! that never left — a warning-level contributor is held attached for
//! context, and the close paths only ever touch the `incidents` row. Those
//! rows outlive their incident, so "is this issue in an open incident?" has
//! to consult the incident's `closed_at`, not just `left_at`. Reading
//! `left_at` alone makes a stranded issue look permanently attached, and an
//! issue that already appears attached never opens an incident when it
//! fails: the server goes red with nothing paging.

use commons_types::status::CheckResult;
use database::issues::NewEvent;
use diesel::prelude::*;
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

/// Server in a fresh group whose linger window is zero, so a recovery
/// closes the incident on the spot rather than leaving it to the sweep.
async fn insert_grouped_server(conn: &mut diesel_async::AsyncPgConnection) -> (Uuid, Uuid) {
	let group: RowId = sql_query(
		"INSERT INTO server_groups (name, slack_close_delay) \
		 VALUES ('stranded-group', INTERVAL '0') RETURNING id",
	)
	.get_result(conn)
	.await
	.expect("group");
	let server: RowId = sql_query(
		"INSERT INTO servers (host, group_id) VALUES ('http://stranded.invalid/', $1) RETURNING id",
	)
	.bind::<sql_types::Uuid, _>(group.id)
	.get_result(conn)
	.await
	.expect("server");
	(group.id, server.id)
}

async fn save_event(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	r#ref: &str,
	result: CheckResult,
) {
	let active = matches!(
		result,
		CheckResult::Failed | CheckResult::Warning | CheckResult::Broken
	);
	let stamp = database::issues::CheckStateStamp {
		check: r#ref.into(),
		observed: result,
		effective: result,
		escalates: false,
		detail: None,
	};
	NewEvent {
		source: "test".into(),
		r#ref: r#ref.into(),
		description: None,
		message: format!("{ref_} is {result:?}", ref_ = r#ref),
		active: Some(active),
		occurred_at: None,
	}
	.save_with_state(conn, server_id, None, Some(&stamp), false)
	.await
	.expect("save event");
}

async fn open_incident_count(conn: &mut diesel_async::AsyncPgConnection, group_id: Uuid) -> i64 {
	use database::schema::incidents::dsl;
	dsl::incidents
		.filter(dsl::server_group_id.eq(group_id))
		.filter(dsl::closed_at.is_null())
		.count()
		.get_result(conn)
		.await
		.expect("count open incidents")
}

/// Count membership rows for `ref` that are still unstamped, regardless of
/// whether their incident is closed.
async fn unstamped_memberships(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	r#ref: &str,
) -> i64 {
	#[derive(QueryableByName)]
	struct Count {
		#[diesel(sql_type = sql_types::BigInt)]
		n: i64,
	}
	let row: Count = sql_query(
		"SELECT count(*) AS n FROM incident_issues ii \
		 JOIN issues i ON i.id = ii.issue_id \
		 WHERE i.server_id = $1 AND i.ref = $2 AND ii.left_at IS NULL",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(r#ref)
	.get_result(conn)
	.await
	.expect("count memberships");
	row.n
}

/// The regression: a warning contributor stranded by a close must still be
/// able to open a fresh incident when it later fails.
#[tokio::test(flavor = "multi_thread")]
async fn stranded_member_can_open_a_later_incident() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let (group_id, server_id) = insert_grouped_server(&mut conn).await;

		// A failure opens the incident; a warning joins it as a lesser
		// contributor because the target already has one open.
		save_event(
			&mut conn,
			server_id,
			"health/disk_free",
			CheckResult::Failed,
		)
		.await;
		save_event(
			&mut conn,
			server_id,
			"health/pg_tuning",
			CheckResult::Warning,
		)
		.await;
		assert_eq!(
			open_incident_count(&mut conn, group_id).await,
			1,
			"the failure should have opened an incident",
		);

		// The failure recovers. With a zero linger window the incident
		// closes immediately, and the warning contributor is left attached:
		// this is the stranding, reproduced the way production makes it.
		save_event(
			&mut conn,
			server_id,
			"health/disk_free",
			CheckResult::Passed,
		)
		.await;
		assert_eq!(
			open_incident_count(&mut conn, group_id).await,
			0,
			"the recovery should have closed the incident",
		);
		assert_eq!(
			unstamped_memberships(&mut conn, server_id, "health/pg_tuning").await,
			1,
			"the warning contributor should still be attached to the closed incident",
		);

		// The stranded contributor now fails. Its stale membership names a
		// closed incident, so it is not in an open one, and this failure has
		// to open a new incident.
		save_event(
			&mut conn,
			server_id,
			"health/pg_tuning",
			CheckResult::Failed,
		)
		.await;
		assert_eq!(
			open_incident_count(&mut conn, group_id).await,
			1,
			"a failure on a stranded issue must open a new incident",
		);
	})
	.await;
}
