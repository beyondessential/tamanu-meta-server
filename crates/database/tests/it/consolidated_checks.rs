//! `issues::consolidated_checks_latest` — a server's current checks across
//! every source, graded, with the health rollup matching the headline.

use commons_types::status::{CheckResult, HealthState};
use database::check_policies::CheckPolicy;
use database::issues::{CheckFiling, Scope, consolidated_checks_latest, file_check};
use database::statuses::{CANOPY_SOURCE, REACHABILITY_REF};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_server(conn: &mut diesel_async::AsyncPgConnection) -> Uuid {
	let row: RowId = sql_query(
		"INSERT INTO applications (host) VALUES ('http://consolidated.invalid/') RETURNING id",
	)
	.get_result(conn)
	.await
	.expect("insert server");
	row.id
}

fn filing<'a>(
	server_id: Uuid,
	source: &'a str,
	check: &'a str,
	observed: CheckResult,
) -> CheckFiling<'a> {
	CheckFiling {
		source,
		scope: Scope::Application(server_id),
		device_id: None,
		check,
		observed,
		title: None,
		message: "consolidated test",
		detail: None,
		default_ceiling: CheckResult::Failed,
		default_escalates: false,
		documentation: None,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn latest_merges_all_sources_and_matches_rollup() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(
			&mut conn,
			filing(server_id, "alertd", "db", CheckResult::Failed),
		)
		.await
		.expect("file");
		file_check(
			&mut conn,
			filing(server_id, "alertd", "disk", CheckResult::Passed),
		)
		.await
		.expect("file");
		file_check(
			&mut conn,
			filing(server_id, "tamanu", "tasks", CheckResult::Passed),
		)
		.await
		.expect("file");

		let consolidated = consolidated_checks_latest(&mut conn, server_id, None)
			.await
			.expect("consolidated");

		// All three checks from both sources, most urgent first.
		assert_eq!(consolidated.checks.len(), 3);
		assert_eq!(consolidated.checks[0].effective, CheckResult::Failed);
		assert_eq!(consolidated.checks[0].check, "db");
		let sources: std::collections::BTreeSet<&str> = consolidated
			.checks
			.iter()
			.map(|c| c.source.as_str())
			.collect();
		assert!(sources.contains("alertd") && sources.contains("tamanu"));

		// Rollup matches the headline: a failure makes it unhealthy.
		assert_eq!(consolidated.health_state, HealthState::Unhealthy);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn latest_excludes_orphaned_check_states() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		// A failing check whose catalog row is then deleted, stranding its
		// check-state row (the bestool-alertd situation: an `issues` row with
		// no `check_policies` policy — invisible in settings, unmanageable).
		file_check(
			&mut conn,
			filing(
				server_id,
				"bestool-alertd",
				"sync-errors",
				CheckResult::Failed,
			),
		)
		.await
		.expect("file");
		sql_query("DELETE FROM check_policies WHERE source = 'bestool-alertd'")
			.execute(&mut conn)
			.await
			.expect("strand the check-state");

		let consolidated = consolidated_checks_latest(&mut conn, server_id, None)
			.await
			.expect("consolidated");

		// An orphaned check-state (no catalog row) is not a manageable check:
		// it must not surface in the detail view...
		assert!(
			consolidated.checks.is_empty(),
			"orphaned check-state is excluded from the detail view",
		);
		// ...nor drag the health rollup — a server cannot be broken by a check
		// that no longer exists in the catalog.
		assert_eq!(consolidated.health_state, HealthState::Healthy);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn latest_excludes_decommissioned_and_flags_silenced() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		file_check(
			&mut conn,
			filing(server_id, "alertd", "gone", CheckResult::Warning),
		)
		.await
		.expect("file");
		file_check(
			&mut conn,
			filing(server_id, "alertd", "hushed", CheckResult::Warning),
		)
		.await
		.expect("file");

		// Decommission one check; silence the other at server scope.
		sql_query(
			"UPDATE check_policies SET decommissioned_at = now() \
			 WHERE source = 'alertd' AND check_name = 'gone'",
		)
		.execute(&mut conn)
		.await
		.expect("decommission");
		sql_query(
			"INSERT INTO scoped_check_policies (application_id, source, check_name, ceiling, created_by) \
			 VALUES ($1, 'alertd', 'hushed', 'skipped', 'op')",
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.execute(&mut conn)
		.await
		.expect("silence");

		let consolidated = consolidated_checks_latest(&mut conn, server_id, None)
			.await
			.expect("consolidated");

		// The decommissioned check is gone; the silenced one is present but
		// flagged.
		assert_eq!(consolidated.checks.len(), 1);
		assert_eq!(consolidated.checks[0].check, "hushed");
		assert!(consolidated.checks[0].silenced);
		// Silencing caps the effective result to skipped — matching the
		// rollup's exclusion and the snapshot path's ceiling — while the
		// observed result still records what was reported.
		assert_eq!(consolidated.checks[0].effective, CheckResult::Skipped);
		assert_eq!(consolidated.checks[0].observed, Some(CheckResult::Warning));
	})
	.await
}

/// Find the reachability entry among a server's consolidated checks.
fn reachability(
	consolidated: &commons_types::status::ConsolidatedChecks,
) -> Option<&commons_types::status::ConsolidatedCheck> {
	consolidated
		.checks
		.iter()
		.find(|c| c.source == CANOPY_SOURCE && c.check == REACHABILITY_REF)
}

#[tokio::test(flavor = "multi_thread")]
async fn reachability_presents_as_passed_when_it_has_never_degraded() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// The sweep files reachability only while it's degraded, so a server
		// that has never had a reporter go quiet carries no state for it. It
		// still presents — green — so the check and its silence control are
		// there before anything is red.
		CheckPolicy::seed_own_checks(&mut conn)
			.await
			.expect("seed canopy's own checks");
		let server_id = insert_server(&mut conn).await;

		let consolidated = consolidated_checks_latest(&mut conn, server_id, None)
			.await
			.expect("consolidated");

		let check = reachability(&consolidated).expect("reachability presents");
		assert_eq!(check.effective, CheckResult::Passed);
		assert_eq!(check.observed, Some(CheckResult::Passed));
		assert!(!check.silenced);
		// A passing check doesn't count against the server.
		assert_eq!(consolidated.health_state, HealthState::Healthy);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn synthesised_reachability_reflects_a_silence() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::seed_own_checks(&mut conn)
			.await
			.expect("seed canopy's own checks");
		let server_id = insert_server(&mut conn).await;
		sql_query(
			"INSERT INTO scoped_check_policies (application_id, source, check_name, ceiling, created_by) \
			 VALUES ($1, $2, $3, 'skipped', 'op')",
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.bind::<sql_types::Text, _>(CANOPY_SOURCE)
		.bind::<sql_types::Text, _>(REACHABILITY_REF)
		.execute(&mut conn)
		.await
		.expect("silence reachability");

		let consolidated = consolidated_checks_latest(&mut conn, server_id, None)
			.await
			.expect("consolidated");

		// Silenced reads skipped rather than green: the operator turned
		// alerting off, and the check says so.
		let check = reachability(&consolidated).expect("reachability presents");
		assert_eq!(check.effective, CheckResult::Skipped);
		assert!(check.silenced);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_filed_reachability_wins_over_the_synthesised_one() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::seed_own_checks(&mut conn)
			.await
			.expect("seed canopy's own checks");
		let server_id = insert_server(&mut conn).await;
		file_check(
			&mut conn,
			filing(
				server_id,
				CANOPY_SOURCE,
				REACHABILITY_REF,
				CheckResult::Failed,
			),
		)
		.await
		.expect("file");

		let consolidated = consolidated_checks_latest(&mut conn, server_id, None)
			.await
			.expect("consolidated");

		// One entry, not two: the recorded state is the check.
		assert_eq!(consolidated.checks.len(), 1);
		let check = reachability(&consolidated).expect("reachability presents");
		assert_eq!(check.effective, CheckResult::Failed);
		assert_eq!(consolidated.health_state, HealthState::Unhealthy);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn decommissioning_reachability_removes_it() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::seed_own_checks(&mut conn)
			.await
			.expect("seed canopy's own checks");
		let server_id = insert_server(&mut conn).await;
		sql_query(
			"UPDATE check_policies SET decommissioned_at = now() \
			 WHERE source = $1 AND check_name = $2",
		)
		.bind::<sql_types::Text, _>(CANOPY_SOURCE)
		.bind::<sql_types::Text, _>(REACHABILITY_REF)
		.execute(&mut conn)
		.await
		.expect("decommission");

		let consolidated = consolidated_checks_latest(&mut conn, server_id, None)
			.await
			.expect("consolidated");

		// Presentation follows the catalog: a retired check stays retired
		// rather than being conjured back by the fill-in.
		assert!(reachability(&consolidated).is_none());
	})
	.await
}
