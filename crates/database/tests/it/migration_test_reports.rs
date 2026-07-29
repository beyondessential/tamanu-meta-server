//! Recording a migration test and reading the verdict it produces.

use commons_tests::db::TestDb;
use commons_types::backup::{BackupType, RestoreIntent, RunOutcome};
use database::{
	migration_tests::{MigrationTest, NewMigrationTest, Verdict, verdict},
	pg_duration::PgDuration,
	restore::NewBackupRestoreCheck,
	versions::{NewVersion, Version},
};
use diesel::{QueryableByName, SelectableHelper, sql_query, sql_types};
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
