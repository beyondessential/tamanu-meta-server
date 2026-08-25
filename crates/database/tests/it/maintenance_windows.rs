//! Maintenance windows: an operator's declaration that a target is being
//! worked on. While one suspends a target every check on it grades to
//! skipped, its issues leave their incident, and suspension outlasts the
//! window by the settle period so a server that is back but has not
//! reported yet is not called unreachable.

use commons_types::status::CheckResult;
use database::{
	issues::{CheckFiling, Issue, Scope, file_check},
	maintenance_windows::{MaintenanceWindow, SETTLE},
	statuses::CANOPY_SOURCE,
};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

#[derive(QueryableByName)]
struct Count {
	#[diesel(sql_type = sql_types::BigInt)]
	count: i64,
}

async fn insert_group(conn: &mut diesel_async::AsyncPgConnection) -> Uuid {
	let row: RowId = sql_query("INSERT INTO server_groups (name) VALUES ('g') RETURNING id")
		.get_result(conn)
		.await
		.expect("insert group");
	row.id
}

async fn insert_server(conn: &mut diesel_async::AsyncPgConnection, group_id: Option<Uuid>) -> Uuid {
	let row: RowId = sql_query(
		"INSERT INTO servers (host, group_id) VALUES ('http://maint.invalid/', $1) RETURNING id",
	)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group_id)
	.get_result(conn)
	.await
	.expect("insert server");
	row.id
}

fn filing(server_id: Uuid, check: &str, observed: CheckResult) -> CheckFiling<'_> {
	CheckFiling {
		source: CANOPY_SOURCE,
		scope: Scope::Server(server_id),
		device_id: None,
		check,
		observed,
		title: Some("Maintenance test"),
		message: "maintenance test filing",
		detail: None,
		default_ceiling: CheckResult::Failed,
		default_escalates: false,
		documentation: None,
	}
}

async fn state_for(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	check: &str,
) -> Issue {
	Issue::list_by_source_ref(conn, CANOPY_SOURCE, check, &[server_id])
		.await
		.expect("list")
		.into_iter()
		.next()
		.expect("state row filed")
}

/// Incidents on the group that are still open.
async fn open_incidents(conn: &mut diesel_async::AsyncPgConnection, group_id: Uuid) -> i64 {
	let row: Count = sql_query(
		"SELECT COUNT(*) AS count FROM incidents \
		 WHERE server_group_id = $1 AND closed_at IS NULL",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.get_result(conn)
	.await
	.expect("count open incidents");
	row.count
}

/// Issues currently attached to an incident on the group.
async fn live_members(conn: &mut diesel_async::AsyncPgConnection, group_id: Uuid) -> i64 {
	let row: Count = sql_query(
		"SELECT COUNT(*) AS count FROM incident_issues ii \
		 JOIN incidents i ON i.id = ii.incident_id \
		 WHERE i.server_group_id = $1 AND ii.left_at IS NULL",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.get_result(conn)
	.await
	.expect("count live members");
	row.count
}

async fn backdate_expected_end(
	conn: &mut diesel_async::AsyncPgConnection,
	id: Uuid,
	ago: SignedDuration,
) {
	sql_query("UPDATE maintenance_windows SET expected_end = $2 WHERE id = $1")
		.bind::<sql_types::Uuid, _>(id)
		.bind::<sql_types::Timestamptz, _>(jiff_diesel::Timestamp::from(Timestamp::now() - ago))
		.execute(conn)
		.await
		.expect("backdate");
}

fn in_an_hour() -> Timestamp {
	Timestamp::now() + SignedDuration::from_hours(1)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_window_grades_every_check_on_its_target_to_skipped() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, None).await;
		MaintenanceWindow::declare(
			&mut conn,
			Scope::Server(server_id),
			in_an_hour(),
			Some("upgrading"),
			Some("op"),
		)
		.await
		.expect("declare");

		file_check(
			&mut conn,
			filing(server_id, "reachability", CheckResult::Failed),
		)
		.await
		.expect("file");

		let state = state_for(&mut conn, server_id, "reachability").await;
		assert_eq!(
			state.observed_result,
			Some(CheckResult::Failed),
			"the observation is recorded through the window"
		);
		assert_eq!(
			state.effective_result,
			Some(CheckResult::Skipped),
			"a window grades the check to skipped, as a silence does"
		);
		assert!(!state.active);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn declaring_takes_the_target_out_of_its_open_incident() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group_id = insert_group(&mut conn).await;
		let server_id = insert_server(&mut conn, Some(group_id)).await;

		file_check(
			&mut conn,
			filing(server_id, "reachability", CheckResult::Failed),
		)
		.await
		.expect("file");
		assert_eq!(
			open_incidents(&mut conn, group_id).await,
			1,
			"a failure with no window opens an incident"
		);

		MaintenanceWindow::declare(
			&mut conn,
			Scope::Server(server_id),
			in_an_hour(),
			None,
			Some("op"),
		)
		.await
		.expect("declare");

		assert_eq!(
			live_members(&mut conn, group_id).await,
			0,
			"the issue leaves the incident when the window is declared"
		);
		assert_eq!(
			open_incidents(&mut conn, group_id).await,
			0,
			"nothing else holds the incident open, so it closes immediately"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_group_window_covers_its_servers_and_the_group_itself() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group_id = insert_group(&mut conn).await;
		let server_id = insert_server(&mut conn, Some(group_id)).await;
		MaintenanceWindow::declare(
			&mut conn,
			Scope::Group(group_id),
			in_an_hour(),
			None,
			Some("op"),
		)
		.await
		.expect("declare");

		file_check(
			&mut conn,
			filing(server_id, "reachability", CheckResult::Failed),
		)
		.await
		.expect("file server check");
		assert_eq!(
			state_for(&mut conn, server_id, "reachability")
				.await
				.effective_result,
			Some(CheckResult::Skipped),
			"a group's window covers the checks of every server in it"
		);

		let mut group_filing = filing(server_id, "backup-staleness", CheckResult::Failed);
		group_filing.scope = Scope::Group(group_id);
		file_check(&mut conn, group_filing)
			.await
			.expect("file group check");
		assert_eq!(
			open_incidents(&mut conn, group_id).await,
			0,
			"the group's own checks are covered too, so nothing opens"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_window_ends_at_its_expected_end_and_suspension_outlasts_it() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, None).await;
		let window = MaintenanceWindow::declare(
			&mut conn,
			Scope::Server(server_id),
			in_an_hour(),
			None,
			Some("op"),
		)
		.await
		.expect("declare");

		// A minute past its expected end: the sweep ends it, stamping the
		// end at the expected end rather than at now.
		backdate_expected_end(&mut conn, window.id, SignedDuration::from_mins(1)).await;
		let (ended, settled) = MaintenanceWindow::sweep(&mut conn).await.expect("sweep");
		assert_eq!((ended, settled), (1, 0), "ended, and not yet settled");

		let ended = MaintenanceWindow::get(&mut conn, window.id)
			.await
			.expect("get");
		assert!(ended.ended_at.is_some(), "the expected end ends the window");
		assert_eq!(
			ended.ended_by, None,
			"nobody lifted it; its expected end passed"
		);
		assert!(
			MaintenanceWindow::suspends(&mut conn, Some(server_id), None)
				.await
				.expect("suspends"),
			"suspension runs on through the settle period"
		);

		// Past the settle period now.
		backdate_expected_end(&mut conn, window.id, SETTLE + SignedDuration::from_mins(1)).await;
		sql_query("UPDATE maintenance_windows SET ended_at = expected_end WHERE id = $1")
			.bind::<sql_types::Uuid, _>(window.id)
			.execute(&mut conn)
			.await
			.expect("backdate end");
		assert!(
			!MaintenanceWindow::suspends(&mut conn, Some(server_id), None)
				.await
				.expect("suspends"),
			"the settle period has elapsed, so the target is watched again"
		);

		let (_, settled) = MaintenanceWindow::sweep(&mut conn).await.expect("sweep");
		assert_eq!(settled, 1, "the settled window is claimed exactly once");
		let (_, again) = MaintenanceWindow::sweep(&mut conn).await.expect("sweep");
		assert_eq!(again, 0, "and not claimed twice");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failure_after_the_settle_period_opens_an_incident_again() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group_id = insert_group(&mut conn).await;
		let server_id = insert_server(&mut conn, Some(group_id)).await;
		let window = MaintenanceWindow::declare(
			&mut conn,
			Scope::Server(server_id),
			in_an_hour(),
			None,
			Some("op"),
		)
		.await
		.expect("declare");

		file_check(
			&mut conn,
			filing(server_id, "reachability", CheckResult::Failed),
		)
		.await
		.expect("file during the window");
		assert_eq!(open_incidents(&mut conn, group_id).await, 0);

		backdate_expected_end(&mut conn, window.id, SETTLE + SignedDuration::from_mins(1)).await;
		MaintenanceWindow::sweep(&mut conn).await.expect("sweep");

		file_check(
			&mut conn,
			filing(server_id, "reachability", CheckResult::Failed),
		)
		.await
		.expect("file after settling");
		assert_eq!(
			state_for(&mut conn, server_id, "reachability")
				.await
				.effective_result,
			Some(CheckResult::Failed),
			"grading is normal again once the settle period has elapsed"
		);
		assert_eq!(
			open_incidents(&mut conn, group_id).await,
			1,
			"anything still failing contributes from then on"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn declaring_over_an_open_window_amends_it() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, None).await;
		let first = MaintenanceWindow::declare(
			&mut conn,
			Scope::Server(server_id),
			in_an_hour(),
			Some("upgrading"),
			Some("op"),
		)
		.await
		.expect("declare");

		let later = Timestamp::now() + SignedDuration::from_hours(3);
		let second = MaintenanceWindow::declare(
			&mut conn,
			Scope::Server(server_id),
			later,
			Some("still upgrading"),
			Some("other"),
		)
		.await
		.expect("amend");

		assert_eq!(second.id, first.id, "a target has at most one open window");
		assert_eq!(second.declared_by.as_deref(), Some("op"));
		assert_eq!(
			second.amended_by.as_deref(),
			Some("other"),
			"the amendment records who made it"
		);
		assert!(second.amended_at.is_some());
		assert!(second.expected_end > first.expected_end);

		let windows = MaintenanceWindow::list_for_scope(&mut conn, Scope::Server(server_id), 10)
			.await
			.expect("list");
		assert_eq!(windows.len(), 1, "and not a second window");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn lifting_records_the_operator_and_is_idempotent() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn, None).await;
		let window = MaintenanceWindow::declare(
			&mut conn,
			Scope::Server(server_id),
			in_an_hour(),
			None,
			Some("op"),
		)
		.await
		.expect("declare");

		let lifted = MaintenanceWindow::lift(&mut conn, window.id, Some("other"))
			.await
			.expect("lift");
		assert_eq!(lifted.ended_by.as_deref(), Some("other"));
		let ended_at = lifted.ended_at.expect("ended");

		let again = MaintenanceWindow::lift(&mut conn, window.id, Some("third"))
			.await
			.expect("lift again");
		assert_eq!(
			again.ended_at,
			Some(ended_at),
			"a window already ended is left as it was"
		);
		assert_eq!(again.ended_by.as_deref(), Some("other"));
	})
	.await
}
