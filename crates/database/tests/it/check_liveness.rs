//! Check liveness & decommissioning at the database layer: the source
//! freshness that drives per-server staleness must ignore decommissioned
//! checks, so a source whose every check is retired stops being expected.

use commons_types::status::CheckResult;
use database::issues::{CheckFiling, FilingScope, Issue, file_check};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

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
