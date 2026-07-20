//! Check liveness & decommissioning at the database layer: the source
//! freshness that drives per-server staleness must ignore decommissioned
//! checks, so a source whose every check is retired stops being expected.

use commons_types::status::{CheckResult, HealthState};
use database::check_policies::CheckPolicy;
use database::issues::{CheckFiling, FilingScope, Issue, file_check, health_from_check_state};
use database::statuses::CANOPY_SOURCE;
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

fn filing_result<'a>(
	server_id: Uuid,
	source: &'a str,
	check: &'a str,
	observed: CheckResult,
) -> CheckFiling<'a> {
	CheckFiling {
		source,
		scope: FilingScope::Server {
			server_id,
			device_id: None,
		},
		check,
		observed,
		title: Some("decommission test"),
		message: "decommission test filing",
		detail: None,
		default_ceiling: CheckResult::Failed,
		default_escalates: false,
		documentation: None,
	}
}

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_server(conn: &mut diesel_async::AsyncPgConnection) -> Uuid {
	let row: RowId =
		sql_query("INSERT INTO servers (host) VALUES ('http://liveness.invalid/') RETURNING id")
			.get_result(conn)
			.await
			.expect("insert server");
	row.id
}

fn filing<'a>(server_id: Uuid, source: &'a str, check: &'a str) -> CheckFiling<'a> {
	CheckFiling {
		source,
		scope: FilingScope::Server {
			server_id,
			device_id: None,
		},
		check,
		observed: CheckResult::Passed,
		title: None,
		message: "liveness test filing",
		detail: None,
		default_ceiling: CheckResult::Warning,
		default_escalates: false,
		documentation: None,
	}
}

async fn decommission(conn: &mut diesel_async::AsyncPgConnection, source: &str, check: &str) {
	sql_query(
		"UPDATE check_policies SET decommissioned_at = now() \
		 WHERE source = $1 AND check_name = $2",
	)
	.bind::<sql_types::Text, _>(source)
	.bind::<sql_types::Text, _>(check)
	.execute(conn)
	.await
	.expect("decommission");
}

async fn source_expected(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	source: &str,
) -> bool {
	Issue::source_freshness(conn, &[server_id])
		.await
		.expect("freshness")
		.iter()
		.any(|(sid, src, _)| *sid == server_id && src == source)
}

#[tokio::test(flavor = "multi_thread")]
async fn source_with_all_checks_decommissioned_drops_out_of_freshness() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(&mut conn, filing(server_id, "alertd", "a"))
			.await
			.expect("file a");
		file_check(&mut conn, filing(server_id, "alertd", "b"))
			.await
			.expect("file b");
		assert!(source_expected(&mut conn, server_id, "alertd").await);

		decommission(&mut conn, "alertd", "a").await;
		decommission(&mut conn, "alertd", "b").await;
		assert!(
			!source_expected(&mut conn, server_id, "alertd").await,
			"a source whose every check is decommissioned is no longer expected",
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn source_with_a_live_check_stays_expected() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(&mut conn, filing(server_id, "alertd", "a"))
			.await
			.expect("file a");
		file_check(&mut conn, filing(server_id, "alertd", "b"))
			.await
			.expect("file b");

		decommission(&mut conn, "alertd", "a").await;
		assert!(
			source_expected(&mut conn, server_id, "alertd").await,
			"a source with any live check is still expected",
		);
	})
	.await
}

#[derive(QueryableByName)]
struct PolicyRow {
	#[diesel(sql_type = sql_types::Bool)]
	last_seen_present: bool,
	#[diesel(sql_type = sql_types::Bool)]
	decommissioned: bool,
	#[diesel(sql_type = sql_types::Text)]
	ceiling: String,
	#[diesel(sql_type = sql_types::Bool)]
	reviewed: bool,
}

async fn policy(
	conn: &mut diesel_async::AsyncPgConnection,
	source: &str,
	check: &str,
) -> PolicyRow {
	sql_query(
		"SELECT last_seen IS NOT NULL AS last_seen_present, \
		 decommissioned_at IS NOT NULL AS decommissioned, \
		 ceiling, reviewed_at IS NOT NULL AS reviewed \
		 FROM check_policies WHERE source = $1 AND check_name = $2",
	)
	.bind::<sql_types::Text, _>(source)
	.bind::<sql_types::Text, _>(check)
	.get_result(conn)
	.await
	.expect("policy row")
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_stamps_last_seen() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(&mut conn, filing(server_id, "alertd", "a"))
			.await
			.expect("file");
		assert!(
			!policy(&mut conn, "alertd", "a").await.last_seen_present,
			"last_seen is not stamped on ingestion",
		);

		CheckPolicy::reconcile_liveness(&mut conn)
			.await
			.expect("reconcile");
		assert!(
			policy(&mut conn, "alertd", "a").await.last_seen_present,
			"reconcile stamps last_seen from check state",
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_ignores_synthetic_sources() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(&mut conn, filing(server_id, "canopy", "reachability"))
			.await
			.expect("file");

		CheckPolicy::reconcile_liveness(&mut conn)
			.await
			.expect("reconcile");
		assert!(
			!policy(&mut conn, "canopy", "reachability")
				.await
				.last_seen_present,
			"synthetic canopy checks are not tracked for liveness",
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_reanimates_a_reported_decommissioned_check() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(&mut conn, filing(server_id, "alertd", "a"))
			.await
			.expect("file");
		// Retire it in the past, with an operator-adjusted, reviewed policy.
		sql_query(
			"UPDATE check_policies SET decommissioned_at = now() - interval '1 day', \
			 decommissioned_by = 'op', ceiling = 'failed', reviewed_at = now(), reviewed_by = 'op' \
			 WHERE source = 'alertd' AND check_name = 'a'",
		)
		.execute(&mut conn)
		.await
		.expect("decommission");

		// It reports again.
		file_check(&mut conn, filing(server_id, "alertd", "a"))
			.await
			.expect("re-report");
		CheckPolicy::reconcile_liveness(&mut conn)
			.await
			.expect("reconcile");

		let p = policy(&mut conn, "alertd", "a").await;
		assert!(!p.decommissioned, "a re-reported check is re-animated");
		assert_eq!(p.ceiling, "warning", "re-animated at the warning ceiling");
		assert!(!p.reviewed, "re-animated pending operator review");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn decommission_resolves_states_and_marks_catalog() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(
			&mut conn,
			filing_result(server_id, "alertd", "x", CheckResult::Failed),
		)
		.await
		.expect("file");
		assert_eq!(
			health_from_check_state(&mut conn, &[(server_id, None)])
				.await
				.expect("rollup")
				.get(&server_id)
				.copied()
				.unwrap_or(HealthState::Healthy),
			HealthState::Unhealthy,
		);

		CheckPolicy::decommission(&mut conn, "alertd", "x", "op")
			.await
			.expect("decommission");

		assert!(policy(&mut conn, "alertd", "x").await.decommissioned);
		let state = Issue::list_by_source_ref(&mut conn, "alertd", "x", &[server_id])
			.await
			.expect("state")
			.into_iter()
			.next()
			.expect("state row");
		assert!(
			state.resolved_at.is_some(),
			"decommissioning resolves the check's states",
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn decommission_clears_stale_when_source_fully_retired() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		// The source's only check, plus a live per-server staleness issue.
		file_check(
			&mut conn,
			filing_result(server_id, "alertd", "x", CheckResult::Passed),
		)
		.await
		.expect("file");
		file_check(
			&mut conn,
			filing_result(
				server_id,
				CANOPY_SOURCE,
				"stale/alertd",
				CheckResult::Failed,
			),
		)
		.await
		.expect("file stale");

		CheckPolicy::decommission(&mut conn, "alertd", "x", "op")
			.await
			.expect("decommission");

		let stale =
			Issue::list_by_source_ref(&mut conn, CANOPY_SOURCE, "stale/alertd", &[server_id])
				.await
				.expect("stale")
				.into_iter()
				.next()
				.expect("stale row");
		assert!(
			stale.resolved_at.is_some(),
			"retiring a source's last check clears its staleness",
		);
	})
	.await
}
