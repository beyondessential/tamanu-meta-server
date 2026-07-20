use commons_types::source::ReachabilityMode;
use commons_types::status::CheckResult;
use database::{
	issues::Issue,
	source_policies::SourcePolicy,
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

/// Insert a monitored server with the given `alert_when_down_for` threshold
/// (seconds, stored as a PostgreSQL `INTERVAL`).
async fn insert_server(
	conn: &mut diesel_async::AsyncPgConnection,
	host: &str,
	alert_when_down_for_secs: i64,
) -> Uuid {
	insert_server_full(conn, host, alert_when_down_for_secs, true).await
}

async fn insert_server_full(
	conn: &mut diesel_async::AsyncPgConnection,
	host: &str,
	alert_when_down_for_secs: i64,
	is_monitored: bool,
) -> Uuid {
	let row: RowId = sql_query(
		r#"
			INSERT INTO servers (host, alert_when_down_for, is_monitored)
			VALUES ($1, ($2 || ' seconds')::INTERVAL, $3)
			RETURNING id
		"#,
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Text, _>(alert_when_down_for_secs.to_string())
	.bind::<sql_types::Bool, _>(is_monitored)
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

/// Seed check state as if `source` reported `check` on the server
/// `minutes_ago` — what ingestion stamps on every report, and what the
/// per-source staleness arm reads freshness from.
async fn insert_check_state(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	source: &str,
	check: &str,
	minutes_ago: i32,
) {
	sql_query(
		r#"
			INSERT INTO issues
			(server_id, source, ref, check_name, observed_result, effective_result,
			 message, active, first_seen, last_seen)
			VALUES ($1, $2, 'health/' || $3, $3, 'passed', 'passed',
			 'seeded', false,
			 NOW() - ($4 || ' minutes')::INTERVAL, NOW() - ($4 || ' minutes')::INTERVAL)
		"#,
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(source)
	.bind::<sql_types::Text, _>(check)
	.bind::<sql_types::Text, _>(minutes_ago.to_string())
	.execute(conn)
	.await
	.expect("insert check state");
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_error_when_threshold_crossed() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// 10-min threshold, status 15 minutes old → cross.
		let id = insert_server(&mut conn, "http://down.invalid/", 600).await;
		insert_status_at(&mut conn, id, 15).await;
		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 1);
		let issue = issue_for(&mut conn, id).await.expect("issue exists");
		assert_eq!(issue.effective_result, Some(CheckResult::Failed));
		assert!(issue.active);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_skips_when_below_threshold() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// 10-min threshold, status 5 minutes old → still fresh.
		let id = insert_server(&mut conn, "http://fresh.invalid/", 600).await;
		insert_status_at(&mut conn, id, 5).await;
		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(issue_for(&mut conn, id).await.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_when_no_status_ever() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// No status row at all → infinite downtime → always crosses threshold.
		let id = insert_server(&mut conn, "http://gone.invalid/", 600).await;
		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 1);
		let issue = issue_for(&mut conn, id).await.expect("issue exists");
		assert_eq!(issue.effective_result, Some(CheckResult::Failed));
		assert!(issue.active);
		assert!(
			issue.message.contains("has never reported"),
			"expected a never-reported message, got: {}",
			issue.message
		);
		assert!(issue.message.contains("(threshold 10m)"));
		assert!(!issue.message.contains("106751991167300d"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_skips_unmonitored_server() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// is_monitored = false: never file regardless of how stale the status
		// is. The threshold is preserved so flipping monitoring back on
		// resumes with the chosen value.
		let id = insert_server_full(&mut conn, "http://silenced.invalid/", 600, false).await;
		insert_status_at(&mut conn, id, 120).await;
		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(issue_for(&mut conn, id).await.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_skips_up_server() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://up.invalid/", 600).await;
		insert_status_at(&mut conn, id, 0).await;
		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(issue_for(&mut conn, id).await.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_uses_per_server_threshold() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// Flappy server: long threshold (30 min), 15-min-old status → still ok.
		let flappy = insert_server(&mut conn, "http://flappy.invalid/", 1800).await;
		insert_status_at(&mut conn, flappy, 15).await;
		// Critical server: short threshold (60 s), 15-min-old status → crosses.
		let critical = insert_server(&mut conn, "http://critical.invalid/", 60).await;
		insert_status_at(&mut conn, critical, 15).await;

		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 1);
		assert!(issue_for(&mut conn, flappy).await.is_none());
		assert!(issue_for(&mut conn, critical).await.is_some());
	})
	.await
}

#[derive(QueryableByName)]
struct RowSecs {
	#[diesel(sql_type = sql_types::Float8)]
	seconds: f64,
}

/// Freshly-inserted servers inherit the column default of 10 minutes.
#[tokio::test(flavor = "multi_thread")]
async fn new_servers_default_to_ten_minutes() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		sql_query("INSERT INTO servers (host) VALUES ('http://new.invalid/')")
			.execute(&mut conn)
			.await
			.expect("insert default");

		let row: RowSecs = sql_query(
			r#"
				SELECT EXTRACT(EPOCH FROM alert_when_down_for)::float8 AS seconds
				FROM servers
				WHERE host = 'http://new.invalid/'
			"#,
		)
		.get_result(&mut conn)
		.await
		.expect("read default");
		assert_eq!(row.seconds as i64, 600);
	})
	.await
}

/// Non-positive durations are rejected at the DB level by a CHECK
/// constraint — "off" is now `is_monitored = false`, not a zero threshold.
#[tokio::test(flavor = "multi_thread")]
async fn check_constraint_forbids_non_positive_duration() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		for bad in ["INTERVAL '-1 second'", "INTERVAL '0'"] {
			let res = sql_query(&format!(
				"INSERT INTO servers (host, alert_when_down_for) \
				 VALUES ('http://bad.invalid/', {bad})"
			))
			.execute(&mut conn)
			.await;
			assert!(res.is_err(), "expected CHECK constraint to reject {bad}",);
		}
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_warns_when_an_on_source_is_stale() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// Reachable via a fresh source, but another `on` source has gone
		// quiet (15m > 10m threshold) → reachability warns, naming it.
		let id = insert_server(&mut conn, "http://quiet-source.invalid/", 600).await;
		insert_check_state(&mut conn, id, "alertd", "db", 15).await;
		insert_check_state(&mut conn, id, "otheragent", "ping", 2).await;

		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 1);
		let issue = issue_for(&mut conn, id).await.expect("reachability issue");
		assert!(issue.active);
		assert_eq!(issue.observed_result, Some(CheckResult::Warning));
		assert!(issue.message.contains("alertd"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_passes_when_all_sources_fresh() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://fresh-sources.invalid/", 600).await;
		insert_check_state(&mut conn, id, "alertd", "db", 2).await;
		insert_check_state(&mut conn, id, "otheragent", "ping", 3).await;

		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(issue_for(&mut conn, id).await.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_fails_when_every_source_is_stale() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://all-stale.invalid/", 600).await;
		insert_check_state(&mut conn, id, "alertd", "db", 20).await;
		insert_check_state(&mut conn, id, "otheragent", "ping", 30).await;

		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 1);
		let issue = issue_for(&mut conn, id).await.expect("reachability issue");
		assert!(issue.active);
		assert_eq!(issue.observed_result, Some(CheckResult::Failed));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_quiet_source_stale_does_not_warn() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://quiet-mode.invalid/", 600).await;
		insert_check_state(&mut conn, id, "alertd", "db", 2).await; // fresh, on
		insert_check_state(&mut conn, id, "legacyagent", "beat", 30).await; // stale
		SourcePolicy::set_reachability(&mut conn, "legacyagent", ReachabilityMode::Quiet)
			.await
			.expect("set quiet");

		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(
			issue_for(&mut conn, id).await.is_none(),
			"a quiet source going stale raises no warning while another source is fresh"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_quiet_only_server_still_unreachable() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://quiet-only.invalid/", 600).await;
		insert_check_state(&mut conn, id, "legacyagent", "beat", 30).await; // stale
		SourcePolicy::set_reachability(&mut conn, "legacyagent", ReachabilityMode::Quiet)
			.await
			.expect("set quiet");

		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 1);
		let issue = issue_for(&mut conn, id).await.expect("reachability issue");
		assert_eq!(
			issue.observed_result,
			Some(CheckResult::Failed),
			"a quiet source still counts toward unreachable when it's the only one"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_off_source_is_excluded() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		// An `off` source is ignored entirely; with a fresh status row and no
		// other counted source, reachability passes.
		let id = insert_server(&mut conn, "http://off-mode.invalid/", 600).await;
		insert_status_at(&mut conn, id, 0).await;
		insert_check_state(&mut conn, id, "offagent", "x", 30).await;
		SourcePolicy::set_reachability(&mut conn, "offagent", ReachabilityMode::Off)
			.await
			.expect("set off");

		let filed = Status::sweep_staleness(&mut conn).await.expect("sweep");
		assert_eq!(filed, 0);
		assert!(issue_for(&mut conn, id).await.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_clears_reachability_when_source_reports_again() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://returning.invalid/", 600).await;
		insert_check_state(&mut conn, id, "alertd", "db", 20).await;
		insert_check_state(&mut conn, id, "otheragent", "ping", 2).await;
		Status::sweep_staleness(&mut conn)
			.await
			.expect("first sweep");
		assert!(issue_for(&mut conn, id).await.unwrap().active);

		// The source reports again: ingestion re-stamps its check state.
		sql_query("UPDATE issues SET last_seen = NOW() WHERE server_id = $1 AND source = 'alertd'")
			.bind::<sql_types::Uuid, _>(id)
			.execute(&mut conn)
			.await
			.expect("restamp state");
		Status::sweep_staleness(&mut conn)
			.await
			.expect("second sweep");
		assert!(!issue_for(&mut conn, id).await.unwrap().active);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_closes_issue_when_server_returns() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let id = insert_server(&mut conn, "http://recover.invalid/", 600).await;
		insert_status_at(&mut conn, id, 45).await;
		Status::sweep_staleness(&mut conn)
			.await
			.expect("first sweep");
		assert!(issue_for(&mut conn, id).await.unwrap().active);

		// New status row at "now" → fresh again, sweep should close the issue.
		insert_status_at(&mut conn, id, 0).await;
		Status::sweep_staleness(&mut conn)
			.await
			.expect("second sweep");
		assert!(!issue_for(&mut conn, id).await.unwrap().active);
	})
	.await
}
