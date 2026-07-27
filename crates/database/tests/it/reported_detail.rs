//! Each source's current server-wide detail: how a push replaces it, how the
//! sources resolve into one set of figures, and how long a value lasts.

use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use uuid::Uuid;

use database::reported_detail::ReportedDetail;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_server(conn: &mut AsyncPgConnection) -> Uuid {
	let host = format!("http://detail.invalid/{}", Uuid::new_v4());
	let server: RowId = sql_query("INSERT INTO servers (host) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(host)
		.get_result(conn)
		.await
		.expect("insert server");
	server.id
}

/// A source's report replaces what it reported before, and leaves other
/// sources' reports alone.
// spec: FIG#sourcing
#[tokio::test(flavor = "multi_thread")]
async fn a_report_replaces_its_own_source_only() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;

		ReportedDetail::record(
			&mut conn,
			server_id,
			"alertd",
			&json!({"bestoolVersion": "2.9.1", "pgVersion": "PostgreSQL 16.3 on x86_64"}),
			None,
		)
		.await
		.expect("record alertd");
		ReportedDetail::record(
			&mut conn,
			server_id,
			"tamanu",
			&json!({"uptimeSecs": 42}),
			None,
		)
		.await
		.expect("record tamanu");

		// alertd reports again, dropping pgVersion from its payload.
		ReportedDetail::record(
			&mut conn,
			server_id,
			"alertd",
			&json!({"bestoolVersion": "2.10.5"}),
			None,
		)
		.await
		.expect("re-record alertd");

		let reports = ReportedDetail::for_server(&mut conn, server_id)
			.await
			.expect("read reports");
		assert_eq!(reports.len(), 2, "one row per source, not one per push");

		let figures = ReportedDetail::merge(&reports);
		assert_eq!(
			figures.bestool_version().as_deref(),
			Some("2.10.5"),
			"the newer report supersedes the source's previous one",
		);
		assert_eq!(
			figures.postgres_version(),
			None,
			"a push is the source's whole truth: a field it drops is no longer reported",
		);
		assert_eq!(
			figures.get("uptimeSecs"),
			Some(&json!(42)),
			"the other source's report is untouched",
		);
	})
	.await
}

/// Figures resolve per field across sources: the most recent source to
/// report a field wins it, and a source that doesn't carry it doesn't erase
/// it.
// spec: FIG#sourcing
#[tokio::test(flavor = "multi_thread")]
async fn figures_resolve_per_field_newest_first() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;

		ReportedDetail::record(
			&mut conn,
			server_id,
			"alertd",
			&json!({"pgVersion": "PostgreSQL 16.3 (Visual C++ build 1940), 64-bit", "nodeVersion": "20.11.0"}),
			None,
		)
		.await
		.expect("record alertd");
		// Reported later, and carries none of alertd's fields.
		sql_query(
			"UPDATE server_reported_detail SET reported_at = NOW() - INTERVAL '1 hour' \
			 WHERE server_id = $1",
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.execute(&mut conn)
		.await
		.expect("age alertd's report");
		ReportedDetail::record(&mut conn, server_id, "tamanu", &json!({"uptimeSecs": 42}), None)
			.await
			.expect("record tamanu");

		let figures =
			ReportedDetail::merge(&ReportedDetail::for_server(&mut conn, server_id).await.unwrap());
		assert_eq!(figures.postgres_version().as_deref(), Some("16.3"));
		assert_eq!(figures.platform().as_deref(), Some("Windows"));
		assert_eq!(figures.node_version().as_deref(), Some("20.11.0"));
	})
	.await
}

/// The Munin flag is one of the reported figures, so it holds for as long as
/// the server exists rather than expiring with a lookback — and an explicit
/// later value overrides an earlier one.
// spec: SVC#munin-link
#[tokio::test(flavor = "multi_thread")]
async fn munin_flag_holds_indefinitely() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;

		let munin = async |conn: &mut AsyncPgConnection| {
			ReportedDetail::merge(&ReportedDetail::for_server(conn, server_id).await.unwrap())
				.munin()
		};

		assert_eq!(munin(&mut conn).await, None, "never reported");

		ReportedDetail::record(
			&mut conn,
			server_id,
			"alertd",
			&json!({"munin": true}),
			None,
		)
		.await
		.expect("record munin");
		// Long past any status-history lookback.
		sql_query(
			"UPDATE server_reported_detail SET reported_at = NOW() - INTERVAL '400 days' \
			 WHERE server_id = $1",
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.execute(&mut conn)
		.await
		.expect("age the report");
		assert_eq!(
			munin(&mut conn).await,
			Some(true),
			"a reported flag doesn't expire, however long the server has been quiet",
		);

		// A source that reports nothing about munin leaves it alone...
		ReportedDetail::record(
			&mut conn,
			server_id,
			"tamanu",
			&json!({"uptimeSecs": 42}),
			None,
		)
		.await
		.expect("record tamanu");
		assert_eq!(munin(&mut conn).await, Some(true));

		// ...but an explicit later false overrides it.
		ReportedDetail::record(
			&mut conn,
			server_id,
			"alertd",
			&json!({"munin": false}),
			None,
		)
		.await
		.expect("record munin=false");
		assert_eq!(munin(&mut conn).await, Some(false));
	})
	.await
}

/// Deleting a server takes its reported detail with it, so the fleet read
/// can't surface a server that no longer exists.
// spec: FIG#sourcing
#[tokio::test(flavor = "multi_thread")]
async fn reported_detail_is_deleted_with_its_server() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let server_id = insert_server(&mut conn).await;
		ReportedDetail::record(
			&mut conn,
			server_id,
			"alertd",
			&json!({"munin": true}),
			None,
		)
		.await
		.expect("record");

		sql_query("DELETE FROM servers WHERE id = $1")
			.bind::<sql_types::Uuid, _>(server_id)
			.execute(&mut conn)
			.await
			.expect("delete server");

		assert!(
			ReportedDetail::for_server(&mut conn, server_id)
				.await
				.expect("read reports")
				.is_empty()
		);
	})
	.await
}

/// The fleet read resolves each server's figures independently.
// spec: FIG#fleet-spread
#[tokio::test(flavor = "multi_thread")]
async fn merge_by_server_keeps_servers_apart() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let one = insert_server(&mut conn).await;
		let two = insert_server(&mut conn).await;

		ReportedDetail::record(
			&mut conn,
			one,
			"alertd",
			&json!({"bestoolVersion": "2.10.5"}),
			None,
		)
		.await
		.unwrap();
		ReportedDetail::record(
			&mut conn,
			two,
			"alertd",
			&json!({"bestoolVersion": "2.4.7"}),
			None,
		)
		.await
		.unwrap();

		let merged = ReportedDetail::merge_by_server(ReportedDetail::all(&mut conn).await.unwrap());
		assert_eq!(
			merged.get(&one).unwrap().0.bestool_version().as_deref(),
			Some("2.10.5")
		);
		assert_eq!(
			merged.get(&two).unwrap().0.bestool_version().as_deref(),
			Some("2.4.7")
		);
	})
	.await
}
