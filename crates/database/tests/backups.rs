//! DB-layer tests for the backup-credentials models (`database::backups`).
//! Exercises the model helpers directly against a fresh migrated DB — no HTTP.

use commons_errors::AppError;
use commons_tests::db::TestDb;
use database::diesel_async::AsyncPgConnection;
use database::pg_duration::PgDuration;
use database::{
	BackupConfigStatus, BackupCredentialIssuance, BackupPurpose, BackupRepoSnapshot,
	BackupRepoStats, BackupRequest, BackupRun, BackupType, BackupTypeDefault, MaintenanceKind,
	NewBackupCredentialIssuance, NewBackupRun, NewBackupTypeDefault, NewServerGroupBackupConfig,
	NewServerGroupBackupSchedule, RunOutcome, ServerBackupCapability, ServerGroupBackupConfig,
	ServerGroupBackupSchedule, backups::BackupMaintenanceRun,
};
use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

// --- seeding helpers (raw SQL, matching the existing test style) -----------

#[derive(diesel::QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_group(conn: &mut AsyncPgConnection, name: &str) -> Uuid {
	sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(name)
		.get_result::<RowId>(conn)
		.await
		.expect("insert group")
		.id
}

async fn insert_server(conn: &mut AsyncPgConnection, group_id: Uuid) -> Uuid {
	let host = format!("http://test.invalid/{}", Uuid::new_v4());
	sql_query("INSERT INTO servers (host, kind, group_id) VALUES ($1, 'central', $2) RETURNING id")
		.bind::<sql_types::Text, _>(host)
		.bind::<sql_types::Uuid, _>(group_id)
		.get_result::<RowId>(conn)
		.await
		.expect("insert server")
		.id
}

async fn insert_device(conn: &mut AsyncPgConnection) -> Uuid {
	sql_query("INSERT INTO devices (role) VALUES ('server') RETURNING id")
		.get_result::<RowId>(conn)
		.await
		.expect("insert device")
		.id
}

fn retention() -> serde_json::Value {
	serde_json::json!({"keep_daily": 7, "keep_weekly": 4, "keep_monthly": 6})
}

fn new_config(group_id: Uuid, region: Option<&str>) -> NewServerGroupBackupConfig {
	NewServerGroupBackupConfig {
		group_id,
		bucket: "bes-kopia-backups-test".into(),
		prefix: String::new(),
		target_role_arn: "arn:aws:iam::123456789012:role/canopy-backups-test".into(),
		maintenance_role_arn: "arn:aws:iam::123456789012:role/canopy-maint-test".into(),
		region: region.map(Into::into),
		repo_password_ref: "kopia-repo-pw-test".into(),
		status: BackupConfigStatus::Provisioning,
		mode: commons_types::backup::BackupRepoMode::FromBirth,
		placement: commons_types::backup::BackupPlacement::External,
	}
}

// --- config -----------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn config_upsert_roundtrip_and_status() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;

		// Insert with NULL region.
		let cfg = ServerGroupBackupConfig::upsert(&mut conn, new_config(group_id, None))
			.await
			.expect("insert config");
		assert_eq!(cfg.region, None);
		assert_eq!(cfg.status, BackupConfigStatus::Provisioning);
		assert_eq!(
			cfg.created_at, cfg.updated_at,
			"fresh row: created == updated"
		);

		// Upsert replaces mutable fields (region now set).
		let cfg = ServerGroupBackupConfig::upsert(
			&mut conn,
			new_config(group_id, Some("ap-southeast-2")),
		)
		.await
		.expect("upsert config");
		assert_eq!(cfg.region.as_deref(), Some("ap-southeast-2"));

		// Status transition + updated_at auto-touch.
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
		let cfg =
			ServerGroupBackupConfig::set_status(&mut conn, group_id, BackupConfigStatus::Ready)
				.await
				.expect("set status");
		assert_eq!(cfg.status, BackupConfigStatus::Ready);
		assert!(
			cfg.updated_at > cfg.created_at,
			"updated_at auto-touched on update"
		);

		assert!(
			ServerGroupBackupConfig::get(&mut conn, group_id)
				.await
				.unwrap()
				.is_some()
		);
		assert!(
			ServerGroupBackupConfig::get(&mut conn, Uuid::new_v4())
				.await
				.unwrap()
				.is_none()
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn type_defaults_retention_must_be_object() {
	TestDb::run(|mut conn, _url| async move {
		// Valid object retention round-trips.
		let td = BackupTypeDefault::upsert(
			&mut conn,
			NewBackupTypeDefault {
				r#type: BackupType::TamanuPostgres,
				default_interval: Some(PgDuration(SignedDuration::from_hours(24))),
				default_retention: retention(),
				auto_enable: true,
				allow_below_floor: false,
			},
		)
		.await
		.expect("insert type default");
		assert_eq!(td.r#type, BackupType::TamanuPostgres);
		assert!(td.auto_enable);

		// A non-object retention is rejected by the jsonb_typeof CHECK.
		let err = BackupTypeDefault::upsert(
			&mut conn,
			NewBackupTypeDefault {
				r#type: BackupType::from("bad"),
				default_interval: None,
				default_retention: serde_json::json!([1, 2, 3]),
				auto_enable: false,
				allow_below_floor: false,
			},
		)
		.await;
		assert!(err.is_err(), "array retention must violate the CHECK");
	})
	.await;
}

// --- FK semantics: plain references, no cascade (archival model) ------------

#[tokio::test(flavor = "multi_thread")]
async fn hard_deleting_a_group_with_backup_rows_is_blocked() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id).await;
		let device_id = insert_device(&mut conn).await;

		ServerGroupBackupConfig::upsert(&mut conn, new_config(group_id, None))
			.await
			.unwrap();
		BackupRun::record(
			&mut conn,
			new_run(
				Uuid::new_v4(),
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Backup,
				RunOutcome::Success,
			),
		)
		.await
		.unwrap();

		// Groups/servers are archived, never hard-deleted; the FKs are plain
		// references (no cascade), so a hard DELETE is blocked rather than
		// silently cascading the config/audit away.
		let res = sql_query("DELETE FROM server_groups WHERE id = $1")
			.bind::<sql_types::Uuid, _>(group_id)
			.execute(&mut conn)
			.await;
		assert!(
			res.is_err(),
			"FK restrict must block deleting a referenced group"
		);
	})
	.await;
}

// --- backup_runs ------------------------------------------------------------

fn new_run(
	id: Uuid,
	device_id: Uuid,
	group_id: Uuid,
	server_id: Option<Uuid>,
	purpose: BackupPurpose,
	outcome: RunOutcome,
) -> NewBackupRun {
	NewBackupRun {
		id,
		device_id,
		group_id,
		server_id,
		r#type: BackupType::TamanuPostgres,
		purpose,
		outcome,
		error: None,
		bytes_uploaded: Some(42),
		snapshot_id: Some("kopia-snap-1".into()),
		s3_sent_raw_bytes: None,
		s3_sent_payload_bytes: None,
		s3_received_raw_bytes: None,
		s3_received_payload_bytes: None,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_run_client_uuid_and_duplicate_is_conflict() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id).await;
		let device_id = insert_device(&mut conn).await;

		let run_id = Uuid::new_v4();
		let run = BackupRun::record(
			&mut conn,
			new_run(
				run_id,
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Backup,
				RunOutcome::Success,
			),
		)
		.await
		.expect("first insert");
		assert_eq!(run.id, run_id, "client-supplied UUID is the PK");

		// Re-inserting the same UUID is a clean Conflict, not a panic.
		let err = BackupRun::record(
			&mut conn,
			new_run(
				run_id,
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Backup,
				RunOutcome::Failure,
			),
		)
		.await
		.expect_err("duplicate must error");
		assert!(matches!(err, AppError::Conflict(_)), "got {err:?}");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn latest_success_ignores_restore_and_failure() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id).await;
		let device_id = insert_device(&mut conn).await;
		let pg = BackupType::TamanuPostgres;

		// Oldest: a real successful backup.
		let good = Uuid::new_v4();
		BackupRun::record(
			&mut conn,
			new_run(
				good,
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Backup,
				RunOutcome::Success,
			),
		)
		.await
		.unwrap();
		// Newer: a successful *restore* — must NOT reset backup staleness.
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
		BackupRun::record(
			&mut conn,
			new_run(
				Uuid::new_v4(),
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Restore,
				RunOutcome::Success,
			),
		)
		.await
		.unwrap();
		// Newer still: a *failed* backup — also must not count.
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
		BackupRun::record(
			&mut conn,
			new_run(
				Uuid::new_v4(),
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Backup,
				RunOutcome::Failure,
			),
		)
		.await
		.unwrap();

		let latest = BackupRun::latest_success_for_server(&mut conn, server_id, &pg)
			.await
			.unwrap()
			.expect("a successful backup exists");
		assert_eq!(
			latest.id, good,
			"latest successful *backup* is the original, not the restore/failure"
		);

		let map = BackupRun::latest_success_by_server_type_for_group(&mut conn, group_id)
			.await
			.unwrap();
		assert_eq!(map.get(&(server_id, pg)).map(|r| r.id), Some(good));
	})
	.await;
}

// --- issuances --------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn issuance_snapshots_bucket_and_orders_newest_first() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let device_id = insert_device(&mut conn).await;

		let mk = |bucket: &str| NewBackupCredentialIssuance {
			device_id,
			group_id,
			r#type: BackupType::TamanuPostgres,
			expires_at: Timestamp::now() + SignedDuration::from_hours(1),
			purpose: BackupPurpose::Backup,
			sts_assumed_role: "arn:aws:iam::123456789012:role/canopy-backups-test".into(),
			sts_request_id: Some("req-1".into()),
			access_key_id: Some("ASIAEXAMPLE".into()),
			bucket: bucket.into(),
			prefix: String::new(),
		};

		let first = BackupCredentialIssuance::record(&mut conn, mk("bucket-at-issue"))
			.await
			.unwrap();
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
		let _second = BackupCredentialIssuance::record(&mut conn, mk("later-bucket"))
			.await
			.unwrap();

		// The snapshot column holds what was passed, independent of any config.
		assert_eq!(first.bucket, "bucket-at-issue");

		let list = BackupCredentialIssuance::list_for_device(&mut conn, device_id, 10)
			.await
			.unwrap();
		assert_eq!(list.len(), 2);
		assert_eq!(list[0].bucket, "later-bucket", "newest first");
		assert_eq!(list[1].bucket, "bucket-at-issue");
	})
	.await;
}

// --- maintenance runs -------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn maintenance_start_finish_bracket() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;

		let id = BackupMaintenanceRun::start(&mut conn, group_id, MaintenanceKind::Full)
			.await
			.unwrap();
		let running = &BackupMaintenanceRun::list_for_group(&mut conn, group_id, 10)
			.await
			.unwrap()[0];
		assert_eq!(running.id, id);
		assert_eq!(running.kind, MaintenanceKind::Full);
		assert!(running.outcome.is_none(), "NULL outcome while running");
		assert!(running.finished_at.is_none());

		BackupMaintenanceRun::finish(&mut conn, id, RunOutcome::Success, None, Some(1024))
			.await
			.unwrap();
		let done = &BackupMaintenanceRun::list_for_group(&mut conn, group_id, 10)
			.await
			.unwrap()[0];
		assert_eq!(done.outcome, Some(RunOutcome::Success));
		assert_eq!(done.bytes_reclaimed, Some(1024));
		assert!(done.finished_at.is_some());
	})
	.await;
}

// --- repo snapshots ---------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn repo_snapshot_upsert_in_place() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id).await;
		let pg = BackupType::TamanuPostgres;
		let source = format!("canopy@{server_id}:/data");

		// Fixed microsecond-precision timestamps: Postgres timestamptz truncates
		// to microseconds, so a nanosecond `Timestamp::now()` wouldn't round-trip
		// to an equal value.
		let t1: Timestamp = "2026-06-10T00:00:00.000001Z".parse().unwrap();
		BackupRepoSnapshot::upsert(
			&mut conn,
			group_id,
			&source,
			Some(server_id),
			Some(&pg),
			Some(t1),
		)
		.await
		.unwrap();
		let rows = BackupRepoSnapshot::list_for_group(&mut conn, group_id)
			.await
			.unwrap();
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].latest_snapshot_at, Some(t1));
		assert_eq!(rows[0].r#type, Some(pg.clone()));

		// Second observation of the same (group, source) updates in place.
		let t2: Timestamp = "2026-06-11T12:34:56.654321Z".parse().unwrap();
		BackupRepoSnapshot::upsert(
			&mut conn,
			group_id,
			&source,
			Some(server_id),
			Some(&pg),
			Some(t2),
		)
		.await
		.unwrap();
		let rows = BackupRepoSnapshot::list_for_group(&mut conn, group_id)
			.await
			.unwrap();
		assert_eq!(rows.len(), 1, "still one row");
		assert_eq!(rows[0].latest_snapshot_at, Some(t2));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn latest_backup_and_last_inspected_for_group() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id).await;
		let device_id = insert_device(&mut conn).await;

		// Nothing reported / inspected yet.
		assert!(
			BackupRun::latest_backup_at_for_group(&mut conn, group_id)
				.await
				.unwrap()
				.is_none()
		);
		assert!(
			BackupRepoSnapshot::last_inspected_at_for_group(&mut conn, group_id)
				.await
				.unwrap()
				.is_none()
		);

		// A failed backup and a successful restore don't count as a backup.
		BackupRun::record(
			&mut conn,
			new_run(
				Uuid::new_v4(),
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Backup,
				RunOutcome::Failure,
			),
		)
		.await
		.unwrap();
		BackupRun::record(
			&mut conn,
			new_run(
				Uuid::new_v4(),
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Restore,
				RunOutcome::Success,
			),
		)
		.await
		.unwrap();
		assert!(
			BackupRun::latest_backup_at_for_group(&mut conn, group_id)
				.await
				.unwrap()
				.is_none(),
			"only successful backups count"
		);

		// A successful backup → its reported_at.
		BackupRun::record(
			&mut conn,
			new_run(
				Uuid::new_v4(),
				device_id,
				group_id,
				Some(server_id),
				BackupPurpose::Backup,
				RunOutcome::Success,
			),
		)
		.await
		.unwrap();
		let runs = BackupRun::list_for_group(&mut conn, group_id, 10)
			.await
			.unwrap();
		let success = runs
			.iter()
			.find(|r| r.purpose == BackupPurpose::Backup && r.outcome == RunOutcome::Success)
			.unwrap();
		assert_eq!(
			BackupRun::latest_backup_at_for_group(&mut conn, group_id)
				.await
				.unwrap(),
			Some(success.reported_at)
		);

		// last-inspected comes from the (inspection-only) snapshots table.
		let t: Timestamp = "2026-06-10T00:00:00.000001Z".parse().unwrap();
		BackupRepoSnapshot::upsert(
			&mut conn,
			group_id,
			&format!("canopy@{server_id}:/data"),
			Some(server_id),
			Some(&BackupType::TamanuPostgres),
			Some(t),
		)
		.await
		.unwrap();
		let snaps = BackupRepoSnapshot::list_for_group(&mut conn, group_id)
			.await
			.unwrap();
		assert_eq!(
			BackupRepoSnapshot::last_inspected_at_for_group(&mut conn, group_id)
				.await
				.unwrap(),
			Some(snaps[0].observed_at)
		);
	})
	.await;
}

// --- repo stats: two independent writers ------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn repo_stats_split_writers_do_not_clobber() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;

		// Inspection writer first, then bucket-bytes writer.
		BackupRepoStats::upsert_repo_fields(
			&mut conn,
			group_id,
			Some(10),
			Some(3),
			Some(1000),
			Some(500),
		)
		.await
		.unwrap();
		BackupRepoStats::upsert_bucket_bytes(&mut conn, group_id, Some(777))
			.await
			.unwrap();
		let s = BackupRepoStats::get(&mut conn, group_id)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			(
				s.snapshot_count,
				s.source_count,
				s.logical_bytes,
				s.physical_bytes,
				s.bucket_bytes
			),
			(Some(10), Some(3), Some(1000), Some(500), Some(777))
		);

		// Bucket-bytes writer must not clobber the repo fields.
		BackupRepoStats::upsert_bucket_bytes(&mut conn, group_id, Some(888))
			.await
			.unwrap();
		let s = BackupRepoStats::get(&mut conn, group_id)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(s.bucket_bytes, Some(888));
		assert_eq!(s.snapshot_count, Some(10), "repo fields preserved");

		// And the inspection writer must not clobber bucket_bytes.
		BackupRepoStats::upsert_repo_fields(
			&mut conn,
			group_id,
			Some(11),
			Some(3),
			Some(1100),
			Some(550),
		)
		.await
		.unwrap();
		let s = BackupRepoStats::get(&mut conn, group_id)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(s.snapshot_count, Some(11));
		assert_eq!(s.bucket_bytes, Some(888), "bucket_bytes preserved");
	})
	.await;
}

// --- capabilities -----------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn capability_register_seeds_once_then_operator_controls() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id).await;
		let pg = BackupType::TamanuPostgres;

		// First registration seeds enabled from the type default.
		let cap = ServerBackupCapability::register(&mut conn, server_id, &pg, true)
			.await
			.unwrap();
		assert!(cap.enabled);

		// Operator turns it off.
		ServerBackupCapability::set_enabled(&mut conn, server_id, &pg, false)
			.await
			.unwrap();

		// Re-registration must NOT re-seed enabled — operator's choice sticks.
		let cap = ServerBackupCapability::register(&mut conn, server_id, &pg, true)
			.await
			.unwrap();
		assert!(!cap.enabled, "re-register keeps the operator-set enabled");

		assert!(
			ServerBackupCapability::list_enabled(&mut conn)
				.await
				.unwrap()
				.is_empty()
		);
		ServerBackupCapability::set_enabled(&mut conn, server_id, &pg, true)
			.await
			.unwrap();
		assert_eq!(
			ServerBackupCapability::list_enabled(&mut conn)
				.await
				.unwrap()
				.len(),
			1
		);
	})
	.await;
}

// --- schedule ---------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn schedule_upsert_and_get() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let pg = BackupType::TamanuPostgres;

		ServerGroupBackupSchedule::upsert(
			&mut conn,
			NewServerGroupBackupSchedule {
				group_id,
				r#type: pg.clone(),
				expected_interval: Some(PgDuration(SignedDuration::from_hours(12))),
				retention: Some(retention()),
				allow_below_floor: false,
			},
		)
		.await
		.unwrap();

		let got = ServerGroupBackupSchedule::get(&mut conn, group_id, &pg)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			got.expected_interval,
			Some(PgDuration(SignedDuration::from_hours(12)))
		);
		assert!(got.retention.is_some());

		// Upsert overrides in place.
		ServerGroupBackupSchedule::upsert(
			&mut conn,
			NewServerGroupBackupSchedule {
				group_id,
				r#type: pg.clone(),
				expected_interval: None,
				retention: None,
				allow_below_floor: false,
			},
		)
		.await
		.unwrap();
		let got = ServerGroupBackupSchedule::get(&mut conn, group_id, &pg)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(got.expected_interval, None);
		assert_eq!(
			ServerGroupBackupSchedule::list_for_group(&mut conn, group_id)
				.await
				.unwrap()
				.len(),
			1
		);
	})
	.await;
}

// --- requests ---------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn requests_enqueue_clear_list() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id).await;
		let pg = BackupType::TamanuPostgres;

		BackupRequest::enqueue(
			&mut conn,
			server_id,
			&pg,
			BackupPurpose::Backup,
			Some("op@bes"),
		)
		.await
		.unwrap();
		// Re-enqueue is an upsert on (server, type, purpose) — still one row.
		BackupRequest::enqueue(
			&mut conn,
			server_id,
			&pg,
			BackupPurpose::Backup,
			Some("op2@bes"),
		)
		.await
		.unwrap();
		let pending = BackupRequest::pending_for_server(&mut conn, server_id)
			.await
			.unwrap();
		assert_eq!(pending.len(), 1);
		assert_eq!(pending[0].requested_by.as_deref(), Some("op2@bes"));

		// A different purpose is a distinct row.
		BackupRequest::enqueue(&mut conn, server_id, &pg, BackupPurpose::Restore, None)
			.await
			.unwrap();
		assert_eq!(
			BackupRequest::pending_for_server(&mut conn, server_id)
				.await
				.unwrap()
				.len(),
			2
		);

		BackupRequest::clear(&mut conn, server_id, &pg, BackupPurpose::Backup)
			.await
			.unwrap();
		let pending = BackupRequest::pending_for_server(&mut conn, server_id)
			.await
			.unwrap();
		assert_eq!(pending.len(), 1);
		assert_eq!(pending[0].purpose, BackupPurpose::Restore);
	})
	.await;
}
