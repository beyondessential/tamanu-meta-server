//! `issues::consolidated_checks_latest` — a server's current checks across
//! every source, graded, with the health rollup matching the headline.

use commons_types::status::{CheckResult, HealthState};
use database::issues::{CheckFiling, FilingScope, consolidated_checks_latest, file_check};
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
		"INSERT INTO servers (host) VALUES ('http://consolidated.invalid/') RETURNING id",
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
		scope: FilingScope::Server {
			server_id,
			device_id: None,
		},
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
			"INSERT INTO scoped_check_policies (server_id, source, check_name, ceiling, created_by) \
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
	})
	.await
}
