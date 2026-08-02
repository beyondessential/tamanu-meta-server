//! Recording where a deployment is going, and what that changes: the planned
//! version becomes what pre-upgrade testing holds the data against.

use commons_tests::db::TestDb;
use commons_types::version::{VersionStatus, VersionStr};
use database::{
	migration_tests::candidate_for,
	reported_detail::ReportedDetail,
	server_groups::ServerGroup,
	servers::Server,
	upgrade_plans::{UpgradePlan, close_met_plans, is_late, planned_target},
	versions::{NewVersion, Version},
};
use diesel::{QueryableByName, SelectableHelper, sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::civil::date;
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn publish(conn: &mut AsyncPgConnection, minor: i32, patch: i32) -> Version {
	diesel::insert_into(database::schema::versions::table)
		.values(NewVersion {
			major: 2,
			minor,
			patch,
			status: VersionStatus::Published,
			changelog: String::new(),
			device_id: None,
		})
		.returning(Version::as_returning())
		.get_result(conn)
		.await
		.expect("publish")
}

/// A group with one server reporting `running`, and the group's cached
/// effective version recomputed through the real path.
async fn group_running(conn: &mut AsyncPgConnection, running: &str) -> (Uuid, Server) {
	let group: RowId = sql_query("INSERT INTO server_groups (name) VALUES ('kamaka') RETURNING id")
		.get_result(conn)
		.await
		.expect("group");
	let server: RowId = sql_query(
		"INSERT INTO servers (host, kind, group_id) VALUES ($1, 'central', $2) RETURNING id",
	)
	.bind::<sql_types::Text, _>("https://central.kamaka.example")
	.bind::<sql_types::Uuid, _>(group.id)
	.get_result(conn)
	.await
	.expect("server");

	let version: VersionStr = running.parse().expect("parse");
	ReportedDetail::record(
		conn,
		server.id,
		"test",
		&serde_json::json!({}),
		Some(&version),
	)
	.await
	.expect("report");
	sql_query(
		"INSERT INTO statuses (server_id, version, healthy, health) VALUES ($1, $2, true, '[]'::jsonb)",
	)
	.bind::<sql_types::Uuid, _>(server.id)
	.bind::<sql_types::Text, _>(running)
	.execute(conn)
	.await
	.expect("status");
	ServerGroup::recompute_version(conn, group.id)
		.await
		.expect("recompute");

	let server = Server::get_by_id(conn, server.id)
		.await
		.expect("get server");
	(group.id, server)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plan_overrides_the_newest_version_as_the_test_target() {
	TestDb::run(|mut conn, _url| async move {
		let (group, server) = group_running(&mut conn, "2.60.0").await;
		let intended = publish(&mut conn, 61, 2).await;
		let newest = publish(&mut conn, 63, 0).await;

		// With no plan, testing aims at the newest published version.
		assert_eq!(
			candidate_for(&mut conn, &server)
				.await
				.expect("candidate")
				.map(|v| v.id),
			Some(newest.id),
		);

		UpgradePlan::record(
			&mut conn,
			group,
			intended.id,
			Some(date(2026, 8, 14)),
			Some("site can absorb 2.61 only"),
			"someone@example.com",
		)
		.await
		.expect("record plan");

		// With one, it aims where the deployment says it is going.
		assert_eq!(
			candidate_for(&mut conn, &server)
				.await
				.expect("candidate")
				.map(|v| v.id),
			Some(intended.id),
			"no restore is spent on a version nobody intends to apply"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn recording_a_plan_retires_the_one_before_it() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.60.0").await;
		let first = publish(&mut conn, 61, 0).await;
		let second = publish(&mut conn, 62, 0).await;

		UpgradePlan::record(&mut conn, group, first.id, None, None, "a@example.com")
			.await
			.expect("first plan");
		UpgradePlan::record(&mut conn, group, second.id, None, None, "a@example.com")
			.await
			.expect("second plan");

		let open = UpgradePlan::open_for_group(&mut conn, group)
			.await
			.expect("open")
			.expect("there is one");
		assert_eq!(
			open.target_version_id, second.id,
			"a group goes one place next"
		);

		let history = UpgradePlan::history_for_group(&mut conn, group)
			.await
			.expect("history");
		assert_eq!(history.len(), 2, "the retired plan is kept");
		let retired = history
			.iter()
			.find(|plan| plan.target_version_id == first.id)
			.expect("the first plan");
		assert!(retired.superseded_at.is_some());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plan_cannot_aim_at_where_the_group_already_is() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.62.0").await;
		let behind = publish(&mut conn, 61, 0).await;

		let refused =
			UpgradePlan::record(&mut conn, group, behind.id, None, None, "a@example.com").await;
		assert!(refused.is_err(), "a plan to go backwards is not a plan");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn canopy_closes_a_plan_once_the_group_arrives() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.60.0").await;
		let target = publish(&mut conn, 61, 0).await;
		UpgradePlan::record(&mut conn, group, target.id, None, None, "a@example.com")
			.await
			.expect("plan");

		assert_eq!(
			close_met_plans(&mut conn).await.expect("sweep"),
			0,
			"still on its way"
		);

		// The deployment lands past the target: further than planned still means
		// the upgrade happened.
		sql_query("UPDATE server_groups SET effective_version = '2.62.0' WHERE id = $1")
			.bind::<sql_types::Uuid, _>(group)
			.execute(&mut conn)
			.await
			.expect("arrive");

		// Through the periodic sweep, which is what runs in production.
		database::backup::sweep(&mut conn).await.expect("sweep");
		assert!(
			UpgradePlan::open_for_group(&mut conn, group)
				.await
				.expect("open")
				.is_none(),
			"a met plan is closed, not left outstanding"
		);
		assert!(
			planned_target(&mut conn, group)
				.await
				.expect("target")
				.is_none(),
			"and stops steering the test target"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_date_that_has_passed_reads_as_late() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.60.0").await;
		let target = publish(&mut conn, 61, 0).await;
		let plan = UpgradePlan::record(
			&mut conn,
			group,
			target.id,
			Some(date(2026, 7, 1)),
			None,
			"a@example.com",
		)
		.await
		.expect("plan");

		assert!(is_late(&plan, date(2026, 7, 30)));
		assert!(!is_late(&plan, date(2026, 6, 30)));

		let undated = UpgradePlan::record(&mut conn, group, target.id, None, None, "a@example.com")
			.await
			.expect("undated plan");
		assert!(
			!is_late(&undated, date(2026, 7, 30)),
			"no date means nothing to be late against"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn amending_a_plan_keeps_it_the_same_plan() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.60.0").await;
		let target = publish(&mut conn, 61, 0).await;

		let plan = UpgradePlan::record(
			&mut conn,
			group,
			target.id,
			Some(date(2026, 7, 1)),
			Some("waiting on the site"),
			"a@example.com",
		)
		.await
		.expect("plan");

		let amended = UpgradePlan::amend(
			&mut conn,
			plan.id,
			Some(date(2026, 8, 14)),
			Some("site confirmed the window"),
			"b@example.com",
		)
		.await
		.expect("amend");

		assert_eq!(amended.id, plan.id, "the same plan, better described");
		assert_eq!(
			amended.target_version_id, target.id,
			"the target is untouched"
		);
		assert_eq!(amended.planned_for, Some(date(2026, 8, 14)));
		assert_eq!(amended.note.as_deref(), Some("site confirmed the window"));
		assert_eq!(amended.amended_by.as_deref(), Some("b@example.com"));
		assert!(amended.amended_at.is_some());
		assert_eq!(
			amended.created_by.as_deref(),
			Some("a@example.com"),
			"who recorded it is not overwritten by who amended it"
		);
		assert!(
			amended.superseded_at.is_none(),
			"an amendment is not a replacement"
		);

		let history = UpgradePlan::history_for_group(&mut conn, group)
			.await
			.expect("history");
		assert_eq!(history.len(), 1, "amending does not add to the history");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn amending_can_clear_the_date_and_note() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.60.0").await;
		let target = publish(&mut conn, 61, 0).await;

		let plan = UpgradePlan::record(
			&mut conn,
			group,
			target.id,
			Some(date(2026, 7, 1)),
			Some("waiting on the site"),
			"a@example.com",
		)
		.await
		.expect("plan");

		let amended = UpgradePlan::amend(&mut conn, plan.id, None, None, "b@example.com")
			.await
			.expect("amend");

		assert!(
			amended.planned_for.is_none(),
			"a date that is no longer expected can be taken off"
		);
		assert!(amended.note.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replaced_plan_is_not_amendable() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.60.0").await;
		let first = publish(&mut conn, 61, 0).await;
		let second = publish(&mut conn, 62, 0).await;

		let replaced = UpgradePlan::record(&mut conn, group, first.id, None, None, "a@example.com")
			.await
			.expect("first plan");
		UpgradePlan::record(&mut conn, group, second.id, None, None, "a@example.com")
			.await
			.expect("second plan");

		let refused = UpgradePlan::amend(
			&mut conn,
			replaced.id,
			Some(date(2026, 8, 1)),
			None,
			"b@example.com",
		)
		.await;
		assert!(
			refused.is_err(),
			"a replaced plan is history and stands as it was"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_met_plan_is_not_amendable() {
	TestDb::run(|mut conn, _url| async move {
		let (group, _server) = group_running(&mut conn, "2.60.0").await;
		let target = publish(&mut conn, 61, 0).await;
		let plan = UpgradePlan::record(&mut conn, group, target.id, None, None, "a@example.com")
			.await
			.expect("plan");

		sql_query("UPDATE server_groups SET effective_version = '2.61.0' WHERE id = $1")
			.bind::<sql_types::Uuid, _>(group)
			.execute(&mut conn)
			.await
			.expect("arrive");
		close_met_plans(&mut conn).await.expect("sweep");

		let refused = UpgradePlan::amend(
			&mut conn,
			plan.id,
			Some(date(2026, 8, 1)),
			None,
			"b@example.com",
		)
		.await;
		assert!(
			refused.is_err(),
			"the upgrade happened; the record of it stands as it was"
		);
	})
	.await
}
