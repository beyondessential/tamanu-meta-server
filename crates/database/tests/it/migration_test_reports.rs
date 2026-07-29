//! Recording a migration test and reading the verdict it produces.

use commons_tests::db::TestDb;
use commons_types::backup::{BackupType, RestoreIntent, RunOutcome};
use database::{
	migration_tests::{MigrationTest, NewMigrationTest, Verdict, verdict},
	pg_duration::PgDuration,
	restore::NewBackupRestoreCheck,
	version_known_issues::VersionKnownIssue,
	versions::{NewVersion, Version},
};
use diesel::{OptionalExtension, QueryableByName, SelectableHelper, sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_group(conn: &mut AsyncPgConnection) -> Uuid {
	let row: RowId = sql_query("INSERT INTO server_groups (name) VALUES ('kamaka') RETURNING id")
		.get_result(conn)
		.await
		.expect("group");
	row.id
}

async fn insert_server(conn: &mut AsyncPgConnection, group_id: Uuid) -> Uuid {
	let row: RowId = sql_query("INSERT INTO servers (host, group_id) VALUES ($1, $2) RETURNING id")
		.bind::<sql_types::Text, _>("https://central.kamaka.example")
		.bind::<sql_types::Uuid, _>(group_id)
		.get_result(conn)
		.await
		.expect("server");
	row.id
}

async fn insert_consumer(conn: &mut AsyncPgConnection) -> Uuid {
	let row: RowId = sql_query("INSERT INTO devices (role) VALUES ('backup-restore') RETURNING id")
		.get_result(conn)
		.await
		.expect("consumer");
	row.id
}

async fn insert_version(conn: &mut AsyncPgConnection, minor: i32) -> Version {
	diesel::insert_into(database::schema::versions::table)
		.values(NewVersion {
			major: 2,
			minor,
			patch: 0,
			status: commons_types::version::VersionStatus::Published,
			changelog: String::new(),
			device_id: None,
		})
		.returning(Version::as_returning())
		.get_result(conn)
		.await
		.expect("version")
}

fn report(consumer: Uuid, group: Uuid, server: Uuid, outcome: RunOutcome) -> NewBackupRestoreCheck {
	NewBackupRestoreCheck {
		replica_id: None,
		consumer_device_id: consumer,
		group_id: group,
		server_id: Some(server),
		r#type: BackupType::TamanuPostgres,
		intent: RestoreIntent::from("migration-test"),
		snapshot_id: Some("snap-x".into()),
		outcome,
		error: None,
		replica_healthy: true,
		postgres_version: Some("18".into()),
		observed_at: Timestamp::now(),
		s3_sent_raw_bytes: None,
		s3_sent_payload_bytes: None,
		s3_received_raw_bytes: None,
		s3_received_payload_bytes: None,
		health_details: None,
		run_id: None,
	}
}

fn secs(n: i64) -> PgDuration {
	PgDuration(SignedDuration::from_secs(n))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_passing_test_records_timings_in_order() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn).await;
		let server = insert_server(&mut conn, group).await;
		let target = insert_version(&mut conn, 63).await;

		let check_id = MigrationTest::record(
			&mut conn,
			report(consumer, group, server, RunOutcome::Success),
			NewMigrationTest {
				target_version_id: target.id,
				total_elapsed: secs(900),
				failed_migration: None,
				data_bytes_before: 200_000_000_000,
				data_bytes_after: 260_000_000_000,
				timings: vec![
					("addIndexToFhirJobs".into(), secs(12)),
					("backfillNoteTypeIds".into(), secs(880)),
				],
			},
		)
		.await
		.expect("record");

		assert_eq!(
			verdict(&mut conn, server, target.id)
				.await
				.expect("verdict"),
			Verdict::Passed
		);

		let timings = MigrationTest::timings(&mut conn, check_id)
			.await
			.expect("timings");
		let names: Vec<&str> = timings.iter().map(|t| t.name.as_str()).collect();
		assert_eq!(
			names,
			vec!["addIndexToFhirJobs", "backfillNoteTypeIds"],
			"kept in the order they ran"
		);
		assert_eq!(
			timings[1].elapsed,
			secs(880),
			"the slow one is attributable"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_named_failing_migration_is_a_failed_verdict() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn).await;
		let server = insert_server(&mut conn, group).await;
		let target = insert_version(&mut conn, 63).await;

		MigrationTest::record(
			&mut conn,
			report(consumer, group, server, RunOutcome::Failure),
			NewMigrationTest {
				target_version_id: target.id,
				total_elapsed: secs(45),
				failed_migration: Some("backfillNoteTypeIds".into()),
				data_bytes_before: 200_000_000_000,
				data_bytes_after: 200_000_000_000,
				timings: vec![("backfillNoteTypeIds".into(), secs(45))],
			},
		)
		.await
		.expect("record");

		assert_eq!(
			verdict(&mut conn, server, target.id)
				.await
				.expect("verdict"),
			Verdict::Failed
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_untested_pair_and_an_untested_version() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn).await;
		let server = insert_server(&mut conn, group).await;
		let tested = insert_version(&mut conn, 63).await;
		let untested = insert_version(&mut conn, 64).await;

		assert_eq!(
			verdict(&mut conn, server, tested.id)
				.await
				.expect("verdict"),
			Verdict::NotTested,
			"nothing reported yet"
		);

		MigrationTest::record(
			&mut conn,
			report(consumer, group, server, RunOutcome::Success),
			NewMigrationTest {
				target_version_id: tested.id,
				total_elapsed: secs(10),
				failed_migration: None,
				data_bytes_before: 1,
				data_bytes_after: 1,
				timings: vec![],
			},
		)
		.await
		.expect("record");

		assert_eq!(
			verdict(&mut conn, server, untested.id)
				.await
				.expect("verdict"),
			Verdict::NotTested,
			"one version's pass says nothing about another's"
		);
	})
	.await
}

#[derive(QueryableByName)]
struct FiledCheck {
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	observed: Option<String>,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	effective: Option<String>,
	#[diesel(sql_type = sql_types::Bool)]
	escalates: bool,
}

async fn migration_check(conn: &mut AsyncPgConnection, server: Uuid) -> Option<FiledCheck> {
	sql_query(
		"SELECT i.observed_result AS observed, i.effective_result AS effective, i.escalates
		 FROM issues i
		 WHERE i.server_id = $1 AND i.ref LIKE 'migration-test:%' AND i.active = true",
	)
	.bind::<sql_types::Uuid, _>(server)
	.get_result(conn)
	.await
	.optional()
	.expect("read filed check")
}

async fn unresolved_issues_for(conn: &mut AsyncPgConnection, server: Uuid) -> i64 {
	#[derive(QueryableByName)]
	struct Count {
		#[diesel(sql_type = sql_types::BigInt)]
		count: i64,
	}
	sql_query(
		"SELECT count(*) AS count FROM version_known_issues
		 WHERE server_id = $1 AND resolved_at IS NULL",
	)
	.bind::<sql_types::Uuid, _>(server)
	.get_result::<Count>(conn)
	.await
	.expect("count known issues")
	.count
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failure_warns_on_the_server_and_holds_the_version_back() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn).await;
		let server = insert_server(&mut conn, group).await;
		let target = insert_version(&mut conn, 63).await;

		MigrationTest::record(
			&mut conn,
			report(consumer, group, server, RunOutcome::Success),
			NewMigrationTest {
				target_version_id: target.id,
				total_elapsed: secs(45),
				failed_migration: Some("backfillNoteTypeIds".into()),
				data_bytes_before: 10,
				data_bytes_after: 10,
				timings: vec![],
			},
		)
		.await
		.expect("record failure");

		let filed = migration_check(&mut conn, server)
			.await
			.expect("a check was filed");
		assert_eq!(
			filed.observed.as_deref(),
			Some("warning"),
			"a warning, not a failure"
		);
		assert_eq!(
			filed.effective.as_deref(),
			Some("warning"),
			"and the policy ceiling keeps it there"
		);
		assert!(!filed.escalates, "does not page against a healthy server");

		assert!(
			!VersionKnownIssue::version_is_ready(&mut conn, 2, 63, 0)
				.await
				.expect("readiness"),
			"the version is held back from rollout"
		);
		assert_eq!(unresolved_issues_for(&mut conn, server).await, 1);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn tomorrows_snapshot_does_not_file_the_issue_twice() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn).await;
		let server = insert_server(&mut conn, group).await;
		let target = insert_version(&mut conn, 63).await;

		for snapshot in ["snap-1", "snap-2"] {
			let mut failing = report(consumer, group, server, RunOutcome::Success);
			failing.snapshot_id = Some(snapshot.into());
			MigrationTest::record(
				&mut conn,
				failing,
				NewMigrationTest {
					target_version_id: target.id,
					total_elapsed: secs(45),
					failed_migration: Some("backfillNoteTypeIds".into()),
					data_bytes_before: 10,
					data_bytes_after: 10,
					timings: vec![],
				},
			)
			.await
			.expect("record failure");
		}

		assert_eq!(
			unresolved_issues_for(&mut conn, server).await,
			1,
			"the same version failing again is the same finding"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_later_pass_recovers_the_check() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn).await;
		let server = insert_server(&mut conn, group).await;
		let target = insert_version(&mut conn, 63).await;

		let mut failing = report(consumer, group, server, RunOutcome::Success);
		failing.snapshot_id = Some("snap-1".into());
		MigrationTest::record(
			&mut conn,
			failing,
			NewMigrationTest {
				target_version_id: target.id,
				total_elapsed: secs(45),
				failed_migration: Some("backfillNoteTypeIds".into()),
				data_bytes_before: 10,
				data_bytes_after: 10,
				timings: vec![],
			},
		)
		.await
		.expect("record failure");
		assert!(migration_check(&mut conn, server).await.is_some());

		let mut passing = report(consumer, group, server, RunOutcome::Success);
		passing.snapshot_id = Some("snap-2".into());
		MigrationTest::record(
			&mut conn,
			passing,
			NewMigrationTest {
				target_version_id: target.id,
				total_elapsed: secs(50),
				failed_migration: None,
				data_bytes_before: 10,
				data_bytes_after: 12,
				timings: vec![],
			},
		)
		.await
		.expect("record pass");

		assert!(
			migration_check(&mut conn, server).await.is_none(),
			"the check recovers once the migrations apply"
		);
	})
	.await
}
