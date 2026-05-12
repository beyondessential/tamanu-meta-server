use commons_types::issue::Severity;
use database::{
	issues::Issue,
	statuses::{CANOPY_SOURCE, REACHABILITY_REF, Status},
};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_server(
	conn: &mut diesel_async::AsyncPgConnection,
	host: &str,
	alert: bool,
) -> Uuid {
	let row: RowId = sql_query(
		r#"
			INSERT INTO servers (host, alert_when_down)
			VALUES ($1, $2)
			RETURNING id
		"#,
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Bool, _>(alert)
	.get_result(conn)
	.await
	.expect("insert server");
	row.id
}

async fn insert_status_at(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	minutes_ago: i32,
) {
	sql_query(
		r#"
			INSERT INTO statuses (server_id, created_at, extra)
			VALUES ($1, NOW() - ($2 || ' minutes')::INTERVAL, '{}'::jsonb)
		"#,
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(minutes_ago.to_string())
	.execute(conn)
	.await
	.expect("insert status");
}

async fn issue_for(conn: &mut diesel_async::AsyncPgConnection, server_id: Uuid) -> Option<Issue> {
	Issue::list_by_source_ref(conn, CANOPY_SOURCE, REACHABILITY_REF, &[server_id])
		.await
		.expect("list issues")
		.into_iter()
		.next()
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_blip_as_notice() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://blip.invalid/", true).await;
		// 5 minutes old → Blip
		insert_status_at(&mut conn, id, 5).await;
		let filed = Status::sweep_reachability(&mut conn).await.expect("sweep");
		assert_eq!(filed, 1);
		let issue = issue_for(&mut conn, id).await.expect("issue exists");
		assert_eq!(issue.severity, Severity::Notice);
		assert!(issue.active);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_down_as_error() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://down.invalid/", true).await;
		// 45 minutes → Down
		insert_status_at(&mut conn, id, 45).await;
		Status::sweep_reachability(&mut conn).await.expect("sweep");
		let issue = issue_for(&mut conn, id).await.expect("issue exists");
		assert_eq!(issue.severity, Severity::Error);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_no_status_as_critical() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// No status row → Gone → Critical.
		let id = insert_server(&mut conn, "http://gone.invalid/", true).await;
		Status::sweep_reachability(&mut conn).await.expect("sweep");
		let issue = issue_for(&mut conn, id).await.expect("issue exists");
		assert_eq!(issue.severity, Severity::Critical);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_skips_when_alert_disabled() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://silenced.invalid/", false).await;
		insert_status_at(&mut conn, id, 45).await;
		let filed = Status::sweep_reachability(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(issue_for(&mut conn, id).await.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_skips_up_server() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://up.invalid/", true).await;
		insert_status_at(&mut conn, id, 0).await;
		let filed = Status::sweep_reachability(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(issue_for(&mut conn, id).await.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_escalates_severity_on_second_pass() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://esc.invalid/", true).await;
		insert_status_at(&mut conn, id, 5).await; // Blip
		Status::sweep_reachability(&mut conn)
			.await
			.expect("first sweep");
		let issue = issue_for(&mut conn, id).await.expect("issue exists");
		assert_eq!(issue.severity, Severity::Notice);
		let id1 = issue.id;

		// Backdate the latest status so it now reads as Down.
		sql_query(
			"UPDATE statuses SET created_at = NOW() - INTERVAL '45 minutes' WHERE server_id = $1",
		)
		.bind::<sql_types::Uuid, _>(id)
		.execute(&mut conn)
		.await
		.expect("backdate");
		Status::sweep_reachability(&mut conn)
			.await
			.expect("second sweep");
		let issue = issue_for(&mut conn, id).await.expect("issue still exists");
		assert_eq!(issue.id, id1, "same issue, escalated in place");
		assert_eq!(issue.severity, Severity::Error);
	})
	.await
}

#[derive(QueryableByName)]
struct RowBool {
	#[diesel(sql_type = sql_types::Bool)]
	alert_when_down: bool,
}

/// Freshly-inserted servers (no explicit `alert_when_down`) inherit the
/// column default. The migration sets it to `true` after backfilling
/// existing rows with `false`, so anything registered post-migration
/// alerts by default.
#[tokio::test(flavor = "multi_thread")]
async fn new_servers_default_to_alerting() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let row: RowBool = sql_query(
			r#"
				INSERT INTO servers (host)
				VALUES ('http://new.invalid/')
				RETURNING alert_when_down
			"#,
		)
		.get_result(&mut conn)
		.await
		.expect("insert default");
		assert!(row.alert_when_down);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_closes_issue_when_server_returns() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://recover.invalid/", true).await;
		insert_status_at(&mut conn, id, 45).await;
		Status::sweep_reachability(&mut conn)
			.await
			.expect("first sweep");
		assert!(issue_for(&mut conn, id).await.unwrap().active);

		// New status row at "now" → Up.
		insert_status_at(&mut conn, id, 0).await;
		Status::sweep_reachability(&mut conn)
			.await
			.expect("second sweep");
		assert!(!issue_for(&mut conn, id).await.unwrap().active);
	})
	.await
}
