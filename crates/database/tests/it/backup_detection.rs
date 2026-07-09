//! DB-level tests for the backup-detection sweeps
//! (`database::backup::staleness` + `database::backup::reconcile`) and the
//! group-level alert path (`database::issues::raise_group_event`).
//!
//! `classify` is exercised as a pure function (no DB); the sweeps and the
//! group-event path are exercised against a fresh migrated DB.
//!
//! Incident-gate assertions deliberately query `incident_issues` for the
//! *specific* issue's open link (joined by `ref`) rather than "does the group
//! have any open incident". `staleness::sweep` always runs `sweep_maintenance`
//! over every `status='ready'` group, and a freshly-seeded group has no recent
//! maintenance run, so it files a group-level `backup-maintenance-stale` that
//! opens an incident on the group regardless. Asserting on the per-server
//! issue's own membership isolates the `is_monitored` gate from that noise.

use commons_tests::db::TestDb;
use commons_types::backup::BackupType;
use commons_types::status::CheckResult;
use database::backup::refs;
use database::backup::staleness::{ScanRow, StalenessVerdict};
use database::diesel_async::AsyncPgConnection;
use database::pg_duration::PgDuration;
use database::{
	BackupConfigStatus, BackupPurpose, BackupRepoSnapshot, BackupRun, NewBackupRun,
	NewServerGroupBackupConfig, NewServerGroupBackupSchedule, RunOutcome, ServerBackupCapability,
	ServerGroupBackupConfig, ServerGroupBackupSchedule,
};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

// --- seeding helpers --------------------------------------------------------

#[derive(QueryableByName)]
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

async fn insert_server(conn: &mut AsyncPgConnection, group_id: Uuid, is_monitored: bool) -> Uuid {
	let host = format!("http://test.invalid/{}", Uuid::new_v4());
	sql_query(
		"INSERT INTO servers (host, kind, group_id, is_monitored) \
		 VALUES ($1, 'central', $2, $3) RETURNING id",
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Bool, _>(is_monitored)
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

fn new_config(group_id: Uuid) -> NewServerGroupBackupConfig {
	NewServerGroupBackupConfig {
		group_id,
		bucket: "bes-kopia-backups-test".into(),
		prefix: String::new(),
		target_role_arn: "arn:aws:iam::123456789012:role/canopy-backups-test".into(),
		maintenance_role_arn: "arn:aws:iam::123456789012:role/canopy-maint-test".into(),
		region: None,
		repo_password_ref: "kopia-repo-pw-test".into(),
		status: BackupConfigStatus::Provisioning,
		mode: commons_types::backup::BackupRepoMode::FromBirth,
		placement: commons_types::backup::BackupPlacement::External,
	}
}

/// Create a `status='ready'` config with `created_at` backdated by `age` so
/// the never/anchor logic can be exercised without sleeping.
async fn insert_ready_config(conn: &mut AsyncPgConnection, group_id: Uuid, age: SignedDuration) {
	ServerGroupBackupConfig::upsert(conn, new_config(group_id))
		.await
		.expect("insert config");
	ServerGroupBackupConfig::set_status(conn, group_id, BackupConfigStatus::Ready)
		.await
		.expect("set ready");
	let secs = age.as_secs();
	sql_query("UPDATE server_group_backup_config SET created_at = NOW() - ($2 || ' seconds')::INTERVAL WHERE group_id = $1")
		.bind::<sql_types::Uuid, _>(group_id)
		.bind::<sql_types::Text, _>(secs.to_string())
		.execute(conn)
		.await
		.expect("backdate config created_at");
}

/// Insert a finished `backup_maintenance_runs` row, backdating both
/// `started_at` and `finished_at` by `finished_age`.
async fn insert_maintenance_run(
	conn: &mut AsyncPgConnection,
	group_id: Uuid,
	kind: &str,
	outcome: &str,
	error: Option<&str>,
	finished_age: SignedDuration,
) {
	let secs = finished_age.as_secs().to_string();
	sql_query(
		"INSERT INTO backup_maintenance_runs (group_id, kind, started_at, finished_at, outcome, error) \
		 VALUES ($1, $2, NOW() - ($5 || ' seconds')::INTERVAL, NOW() - ($5 || ' seconds')::INTERVAL, $3, $4)",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Text, _>(kind)
	.bind::<sql_types::Text, _>(outcome)
	.bind::<sql_types::Nullable<sql_types::Text>, _>(error)
	.bind::<sql_types::Text, _>(secs)
	.execute(conn)
	.await
	.expect("insert maintenance run");
}

async fn insert_schedule(
	conn: &mut AsyncPgConnection,
	group_id: Uuid,
	ty: &BackupType,
	interval: SignedDuration,
) {
	ServerGroupBackupSchedule::upsert(
		conn,
		NewServerGroupBackupSchedule {
			group_id,
			r#type: ty.clone(),
			expected_interval: Some(PgDuration(interval)),
			retention: Some(retention()),
			allow_below_floor: false,
		},
	)
	.await
	.expect("insert schedule");
}

async fn enable_capability(conn: &mut AsyncPgConnection, server_id: Uuid, ty: &BackupType) {
	ServerBackupCapability::register(conn, server_id, ty, true)
		.await
		.expect("register capability");
	ServerBackupCapability::set_enabled(conn, server_id, ty, true)
		.await
		.expect("enable capability");
}

/// Record a `purpose='backup'` success and backdate its `reported_at` by `age`.
async fn insert_backup_success_aged(
	conn: &mut AsyncPgConnection,
	device_id: Uuid,
	group_id: Uuid,
	server_id: Uuid,
	ty: &BackupType,
	age: SignedDuration,
) {
	let id = Uuid::new_v4();
	BackupRun::record(
		conn,
		NewBackupRun {
			id,
			device_id,
			group_id,
			server_id: Some(server_id),
			r#type: ty.clone(),
			purpose: BackupPurpose::Backup,
			outcome: RunOutcome::Success,
			error: None,
			bytes_uploaded: Some(42),
			snapshot_id: Some("kopia-snap".into()),
			s3_sent_raw_bytes: None,
			s3_sent_payload_bytes: None,
			s3_received_raw_bytes: None,
			s3_received_payload_bytes: None,
		},
	)
	.await
	.expect("record backup run");
	let secs = age.as_secs();
	sql_query(
		"UPDATE backup_runs SET reported_at = NOW() - ($2 || ' seconds')::INTERVAL WHERE id = $1",
	)
	.bind::<sql_types::Uuid, _>(id)
	.bind::<sql_types::Text, _>(secs.to_string())
	.execute(conn)
	.await
	.expect("backdate reported_at");
}

// --- issue / incident assertion helpers -------------------------------------

/// The most recent server-scoped issue for `(server, source=canopy, ref)`,
/// if any.
#[derive(QueryableByName, Debug)]
struct IssueRow {
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	effective_result: Option<String>,
	#[diesel(sql_type = sql_types::Bool)]
	active: bool,
}

async fn server_issue(
	conn: &mut AsyncPgConnection,
	server_id: Uuid,
	r#ref: &str,
) -> Option<IssueRow> {
	sql_query(
		"SELECT effective_result, active FROM issues \
		 WHERE server_id = $1 AND source = $2 AND \"ref\" = $3",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(refs::CANOPY_SOURCE)
	.bind::<sql_types::Text, _>(r#ref)
	.get_result::<IssueRow>(conn)
	.await
	.ok()
}

async fn group_issue(
	conn: &mut AsyncPgConnection,
	group_id: Uuid,
	r#ref: &str,
) -> Option<IssueRow> {
	sql_query(
		"SELECT effective_result, active FROM issues \
		 WHERE server_group_id = $1 AND source = $2 AND \"ref\" = $3",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Text, _>(refs::CANOPY_SOURCE)
	.bind::<sql_types::Text, _>(r#ref)
	.get_result::<IssueRow>(conn)
	.await
	.ok()
}

#[derive(QueryableByName)]
struct CountRow {
	#[diesel(sql_type = sql_types::BigInt)]
	n: i64,
}

/// Count open (`left_at IS NULL`) incident links for the issue identified by
/// `(server_id, ref)`. This isolates one per-server issue's incident
/// membership from any group-level incident on the same group.
async fn server_issue_open_links(
	conn: &mut AsyncPgConnection,
	server_id: Uuid,
	r#ref: &str,
) -> i64 {
	sql_query(
		"SELECT COUNT(*) AS n FROM incident_issues ii \
		 JOIN issues i ON i.id = ii.issue_id \
		 WHERE i.server_id = $1 AND i.\"ref\" = $2 AND ii.left_at IS NULL",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(r#ref)
	.get_result::<CountRow>(conn)
	.await
	.expect("count server issue links")
	.n
}

/// Count open incident links for a group-scoped issue identified by
/// `(group_id, ref)`.
async fn group_issue_open_links(conn: &mut AsyncPgConnection, group_id: Uuid, r#ref: &str) -> i64 {
	sql_query(
		"SELECT COUNT(*) AS n FROM incident_issues ii \
		 JOIN issues i ON i.id = ii.issue_id \
		 WHERE i.server_group_id = $1 AND i.\"ref\" = $2 AND ii.left_at IS NULL",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Text, _>(r#ref)
	.get_result::<CountRow>(conn)
	.await
	.expect("count group issue links")
	.n
}

// The per-(server,type) ref suffix that `sweep` folds onto STALENESS/NEVER and
// reconcile folds onto its refs (`:{type}`).
fn typed_ref(base: &str, ty: &BackupType) -> String {
	format!("{base}:{ty}")
}

// ===========================================================================
// Case 1 — `classify` boundaries (pure, no DB)
// ===========================================================================

/// Build a bare `ScanRow` with no associations; `anchor` degenerates to
/// `config_created_at`.
fn scan_row(
	interval: SignedDuration,
	config_created_at: Timestamp,
	last_success_at: Option<Timestamp>,
) -> ScanRow {
	ScanRow {
		server_id: Uuid::nil(),
		group_id: Uuid::nil(),
		device_id: None,
		r#type: BackupType::TamanuPostgres,
		is_monitored: true,
		expected_interval: interval,
		config_created_at,
		min_first_seen: None,
		last_success_at,
	}
}

#[test]
fn classify_boundaries() {
	let now: Timestamp = "2026-06-10T00:00:00Z".parse().unwrap();
	let interval = SignedDuration::from_hours(12);
	// grace = interval * 2 = 24h.
	let old_anchor = now - SignedDuration::from_hours(48);

	// Success well within the interval → Ok (no prior open issue).
	let fresh = now - SignedDuration::from_hours(6);
	assert_eq!(
		scan_row(interval, old_anchor, Some(fresh)).classify(now, false),
		StalenessVerdict::Ok,
		"a recent success is healthy",
	);

	// Success past the grace (>24h) → Stale.
	let stale = now - SignedDuration::from_hours(30);
	assert_eq!(
		scan_row(interval, old_anchor, Some(stale)).classify(now, false),
		StalenessVerdict::Stale,
		"a success older than 2x interval is stale",
	);

	// Just inside the grace (exactly 24h is NOT > grace) → Ok.
	let edge = now - SignedDuration::from_hours(24);
	assert_eq!(
		scan_row(interval, old_anchor, Some(edge)).classify(now, false),
		StalenessVerdict::Ok,
		"grace boundary is inclusive (> grace, not >=)",
	);

	// Never succeeded, anchor older than grace → Never.
	assert_eq!(
		scan_row(interval, old_anchor, None).classify(now, false),
		StalenessVerdict::Never,
		"no success ever, past anchor+grace → never",
	);

	// Never succeeded but freshly authorized (anchor inside grace) → Ok.
	let young_anchor = now - SignedDuration::from_hours(6);
	assert_eq!(
		scan_row(interval, young_anchor, None).classify(now, false),
		StalenessVerdict::Ok,
		"freshly-authorized group must not false-alarm as never",
	);

	// Fresh success while an issue was open → Recovered (clear).
	assert_eq!(
		scan_row(interval, old_anchor, Some(fresh)).classify(now, true),
		StalenessVerdict::Recovered,
		"a fresh success while was_active=true is a recovery",
	);

	// Stale stays Stale even if an issue is already open (no premature clear).
	assert_eq!(
		scan_row(interval, old_anchor, Some(stale)).classify(now, true),
		StalenessVerdict::Stale,
		"still-stale does not clear just because an issue is open",
	);
}

/// `classify` only sees `last_success_at`, which `scan_rows` derives from
/// `purpose='backup' AND outcome='success'`. The DB-level companion
/// `latest_success_ignores_restore_and_failure` in `backups.rs` proves a
/// restore/failure run never populates `last_success_at`. Here we just confirm
/// that an absent success (what a restore-only history yields) classifies as
/// Never past the anchor — i.e. a restore does not reset staleness.
#[test]
fn classify_restore_only_history_is_never() {
	let now: Timestamp = "2026-06-10T00:00:00Z".parse().unwrap();
	let interval = SignedDuration::from_hours(12);
	let old_anchor = now - SignedDuration::from_hours(48);
	assert_eq!(
		scan_row(interval, old_anchor, None).classify(now, false),
		StalenessVerdict::Never,
		"restore-only history (no backup success) is never-backed-up",
	);
}

// ===========================================================================
// Case 2 — staleness::sweep files the right server-level events
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_staleness_for_monitored_server_with_old_success() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12); // grace = 24h.
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id, true).await;
		let device_id = insert_device(&mut conn).await;

		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;
		// Latest success is 48h old → past the 24h grace → stale.
		insert_backup_success_aged(
			&mut conn,
			device_id,
			group_id,
			server_id,
			&pg,
			SignedDuration::from_hours(48),
		)
		.await;

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		assert_eq!(rows.len(), 1, "exactly one scanned (server, type)");
		database::backup::staleness::sweep(&mut conn, &rows)
			.await
			.expect("sweep");

		let sref = typed_ref(refs::STALENESS, &pg);
		let issue = server_issue(&mut conn, server_id, &sref)
			.await
			.expect("staleness issue filed");
		assert_eq!(issue.effective_result.as_deref(), Some("failed"));
		assert!(issue.active, "staleness issue is active");
		// Error opens an incident; monitored server contributes.
		assert_eq!(
			server_issue_open_links(&mut conn, server_id, &sref).await,
			1,
			"monitored staleness opens/joins an incident",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_never_for_server_that_never_succeeded() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12); // grace = 24h.
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id, true).await;

		// Config created 72h ago, no success ever → anchor+grace exceeded.
		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::staleness::sweep(&mut conn, &rows)
			.await
			.expect("sweep");

		let nref = typed_ref(refs::NEVER, &pg);
		let issue = server_issue(&mut conn, server_id, &nref)
			.await
			.expect("never issue filed");
		// Never-reported is a warning (so first-time setup doesn't open an
		// incident); a *missed* backup is the error that pages.
		assert_eq!(issue.effective_result.as_deref(), Some("warning"));
		assert!(issue.active);
		// Staleness ref must NOT be filed when there's never been a success.
		assert!(
			server_issue(&mut conn, server_id, &typed_ref(refs::STALENESS, &pg))
				.await
				.is_none(),
			"never path does not also file backup-staleness",
		);
	})
	.await;
}

// ===========================================================================
// Case 3 — the is_monitored gate (per-server)
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn unmonitored_staleness_records_issue_but_no_incident_link() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12);
		let group_id = insert_group(&mut conn, "g").await;
		// Unmonitored server.
		let server_id = insert_server(&mut conn, group_id, false).await;
		let device_id = insert_device(&mut conn).await;

		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;
		insert_backup_success_aged(
			&mut conn,
			device_id,
			group_id,
			server_id,
			&pg,
			SignedDuration::from_hours(48),
		)
		.await;

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::staleness::sweep(&mut conn, &rows)
			.await
			.expect("sweep");

		let sref = typed_ref(refs::STALENESS, &pg);
		// Issue/event is still recorded unconditionally.
		let issue = server_issue(&mut conn, server_id, &sref)
			.await
			.expect("issue still recorded for unmonitored server");
		assert!(issue.active);
		// ...but it must NOT contribute to any incident (is_monitored gate).
		assert_eq!(
			server_issue_open_links(&mut conn, server_id, &sref).await,
			0,
			"unmonitored staleness must not open/join an incident",
		);
	})
	.await;
}

// ===========================================================================
// Case 4 — reconcile::sweep
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_files_report_gap_when_snapshot_fresh_but_no_report() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12); // grace = 24h.
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id, true).await;

		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;

		// No backup_runs success at all (report path silent), but a FRESH repo
		// snapshot landed for this server's source → report-gap.
		let source = format!("canopy@{server_id}:/data");
		let fresh_snap = Timestamp::now() - SignedDuration::from_hours(2);
		BackupRepoSnapshot::upsert(
			&mut conn,
			group_id,
			&source,
			Some(server_id),
			Some(&pg),
			Some(fresh_snap),
		)
		.await
		.expect("upsert snapshot");

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::reconcile::sweep(&mut conn, &rows)
			.await
			.expect("reconcile sweep");

		let gref = typed_ref(refs::RECONCILE_REPORT_GAP, &pg);
		let issue = server_issue(&mut conn, server_id, &gref)
			.await
			.expect("report-gap issue filed");
		assert_eq!(
			issue.effective_result.as_deref(),
			Some("warning"),
			"report-gap is a warning (non-paging)",
		);
		assert!(issue.active);
		// Warning never opens an incident on its own.
		assert_eq!(
			server_issue_open_links(&mut conn, server_id, &gref).await,
			0,
			"Warning report-gap does not open an incident by itself",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_files_missing_when_report_fresh_but_no_snapshot() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12);
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id, true).await;
		let device_id = insert_device(&mut conn).await;

		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;

		// Fresh report (2h old, within grace)...
		insert_backup_success_aged(
			&mut conn,
			device_id,
			group_id,
			server_id,
			&pg,
			SignedDuration::from_hours(2),
		)
		.await;
		// ...but the only snapshot row is stale-snapshot/fresh-inventory: a repo
		// row observed just now (inventory fresh) whose latest_snapshot_at is
		// old → "report says success but data didn't land".
		let source = format!("canopy@{server_id}:/data");
		let old_snap = Timestamp::now() - SignedDuration::from_hours(72);
		BackupRepoSnapshot::upsert(
			&mut conn,
			group_id,
			&source,
			Some(server_id),
			Some(&pg),
			Some(old_snap),
		)
		.await
		.expect("upsert snapshot");

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::reconcile::sweep(&mut conn, &rows)
			.await
			.expect("reconcile sweep");

		let mref = typed_ref(refs::RECONCILE_MISSING, &pg);
		let issue = group_issue(&mut conn, group_id, &mref)
			.await
			.expect("reconcile-missing issue filed (group-scoped)");
		assert_eq!(issue.effective_result.as_deref(), Some("failed"));
		assert!(issue.active);
		// Group-level Error opens an incident.
		assert_eq!(
			group_issue_open_links(&mut conn, group_id, &mref).await,
			1,
			"reconcile-missing is group-level and opens an incident",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_clears_report_gap_when_report_and_snapshot_agree() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12);
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id, true).await;
		let device_id = insert_device(&mut conn).await;

		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;

		let source = format!("canopy@{server_id}:/data");
		let fresh_snap = Timestamp::now() - SignedDuration::from_hours(2);
		BackupRepoSnapshot::upsert(
			&mut conn,
			group_id,
			&source,
			Some(server_id),
			Some(&pg),
			Some(fresh_snap),
		)
		.await
		.expect("upsert snapshot");

		// First sweep: snapshot fresh, no report → report-gap opens.
		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::reconcile::sweep(&mut conn, &rows)
			.await
			.expect("first reconcile");
		let gref = typed_ref(refs::RECONCILE_REPORT_GAP, &pg);
		assert!(
			server_issue(&mut conn, server_id, &gref)
				.await
				.expect("report-gap open")
				.active,
			"precondition: report-gap is active",
		);

		// Now a fresh report lands too → (report_fresh, snapshot_fresh) clears it.
		insert_backup_success_aged(
			&mut conn,
			device_id,
			group_id,
			server_id,
			&pg,
			SignedDuration::from_hours(1),
		)
		.await;
		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan2");
		database::backup::reconcile::sweep(&mut conn, &rows)
			.await
			.expect("second reconcile");

		let issue = server_issue(&mut conn, server_id, &gref)
			.await
			.expect("report-gap still present (now cleared)");
		assert!(
			!issue.active,
			"report-gap cleared once report and snapshot agree"
		);
		assert_eq!(issue.effective_result.as_deref(), Some("passed"));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_files_size_mismatch_when_reported_size_differs_from_repo() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12);
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id, true).await;
		let device_id = insert_device(&mut conn).await;

		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;

		// A reported run (bytes_uploaded = 42, snapshot_id "kopia-snap")...
		insert_backup_success_aged(
			&mut conn,
			device_id,
			group_id,
			server_id,
			&pg,
			SignedDuration::from_hours(2),
		)
		.await;
		// ...but inspection observed a different logical size for that snapshot.
		BackupRun::backfill_snapshot_logical_bytes(
			&mut conn,
			group_id,
			&[("kopia-snap".into(), 99)],
		)
		.await
		.expect("backfill");

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::reconcile::sweep(&mut conn, &rows)
			.await
			.expect("reconcile sweep");

		let sref = typed_ref(refs::RECONCILE_SIZE_MISMATCH, &pg);
		let issue = server_issue(&mut conn, server_id, &sref)
			.await
			.expect("size-mismatch issue filed");
		assert_eq!(
			issue.effective_result.as_deref(),
			Some("warning"),
			"size-mismatch is a warning (non-paging)",
		);
		assert!(issue.active);
		assert_eq!(
			server_issue_open_links(&mut conn, server_id, &sref).await,
			0,
			"Warning size-mismatch does not open an incident by itself",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_clears_size_mismatch_when_latest_sizes_agree() {
	TestDb::run(|mut conn, _url| async move {
		let pg = BackupType::TamanuPostgres;
		let interval = SignedDuration::from_hours(12);
		let group_id = insert_group(&mut conn, "g").await;
		let server_id = insert_server(&mut conn, group_id, true).await;
		let device_id = insert_device(&mut conn).await;

		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(72)).await;
		insert_schedule(&mut conn, group_id, &pg, interval).await;
		enable_capability(&mut conn, server_id, &pg).await;

		// An older run whose reported size disagrees with the repo → raises.
		insert_backup_success_aged(
			&mut conn,
			device_id,
			group_id,
			server_id,
			&pg,
			SignedDuration::from_hours(5),
		)
		.await;
		BackupRun::backfill_snapshot_logical_bytes(
			&mut conn,
			group_id,
			&[("kopia-snap".into(), 99)],
		)
		.await
		.expect("backfill mismatch");
		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::reconcile::sweep(&mut conn, &rows)
			.await
			.expect("first reconcile");
		let sref = typed_ref(refs::RECONCILE_SIZE_MISMATCH, &pg);
		assert!(
			server_issue(&mut conn, server_id, &sref)
				.await
				.expect("size-mismatch open")
				.active,
			"precondition: size-mismatch is active",
		);

		// A newer run whose reported and observed sizes agree becomes the latest
		// comparable run → clears. (Write-once backfill leaves the older run's
		// recorded size untouched; only the new null row is filled.)
		insert_backup_success_aged(
			&mut conn,
			device_id,
			group_id,
			server_id,
			&pg,
			SignedDuration::from_hours(1),
		)
		.await;
		BackupRun::backfill_snapshot_logical_bytes(
			&mut conn,
			group_id,
			&[("kopia-snap".into(), 42)],
		)
		.await
		.expect("backfill match");
		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan2");
		database::backup::reconcile::sweep(&mut conn, &rows)
			.await
			.expect("second reconcile");

		let issue = server_issue(&mut conn, server_id, &sref)
			.await
			.expect("size-mismatch still present (now cleared)");
		assert!(
			!issue.active,
			"size-mismatch cleared once latest sizes agree"
		);
		assert_eq!(issue.effective_result.as_deref(), Some("passed"));
	})
	.await;
}

// ===========================================================================
// Case 5 — raise_group_event bypasses the is_monitored gate (NON-NEGOTIABLE)
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn group_event_pages_even_when_all_members_unmonitored() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		// Every member is UNMONITORED — the per-server path would never page.
		let _s1 = insert_server(&mut conn, group_id, false).await;
		let _s2 = insert_server(&mut conn, group_id, false).await;

		let stamp = database::issues::CheckStateStamp {
			check: refs::CORRUPTION.into(),
			observed: CheckResult::Failed,
			effective: CheckResult::Failed,
			escalates: true,
			detail: None,
		};
		let issue = database::issues::raise_group_event_with_state(
			&mut conn,
			group_id,
			refs::CORRUPTION,
			None,
			"repo corruption detected",
			true,
			Some(&stamp),
		)
		.await
		.expect("raise group event");

		assert_eq!(issue.server_id, None, "group-scoped issue has no server_id");
		assert_eq!(issue.server_group_id, Some(group_id));
		assert!(issue.active);

		// It opens an incident on the group despite all members being unmonitored.
		assert_eq!(
			group_issue_open_links(&mut conn, group_id, refs::CORRUPTION).await,
			1,
			"group-level event opens an incident regardless of is_monitored",
		);
		let open = database::issues::Incident::list_for_group(&mut conn, group_id, false, 10)
			.await
			.expect("list incidents");
		assert_eq!(open.len(), 1, "exactly one open incident on the group");

		// Recovery: same (source, ref) with active=false and a passed
		// result leaves the incident and closes it.
		let stamp = database::issues::CheckStateStamp {
			check: refs::CORRUPTION.into(),
			observed: CheckResult::Passed,
			effective: CheckResult::Passed,
			escalates: true,
			detail: None,
		};
		database::issues::raise_group_event_with_state(
			&mut conn,
			group_id,
			refs::CORRUPTION,
			None,
			"repo corruption cleared",
			false,
			Some(&stamp),
		)
		.await
		.expect("clear group event");
		assert_eq!(
			group_issue_open_links(&mut conn, group_id, refs::CORRUPTION).await,
			0,
			"recovery removes the issue from its incident",
		);
		let still_open = database::issues::Incident::list_for_group(&mut conn, group_id, false, 10)
			.await
			.expect("list incidents 2");
		assert!(
			still_open.is_empty(),
			"incident auto-closes when its only contributor recovers",
		);
	})
	.await;
}

// ===========================================================================
// Case 6 — maintenance failure (backup-maintenance-error)
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn sweep_files_maintenance_error_when_latest_run_failed_then_clears_on_success() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		// Freshly-created config so maintenance-STALE does NOT also fire — this
		// isolates the failure signal from absence-of-success.
		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(1)).await;

		// The most recently finished run failed an hour ago.
		insert_maintenance_run(
			&mut conn,
			group_id,
			"full",
			"failure",
			Some("kopia maintenance: connection refused"),
			SignedDuration::from_hours(1),
		)
		.await;

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::staleness::sweep(&mut conn, &rows)
			.await
			.expect("sweep");

		let issue = group_issue(&mut conn, group_id, refs::MAINTENANCE_ERROR)
			.await
			.expect("maintenance-error issue filed");
		assert_eq!(issue.effective_result.as_deref(), Some("failed"));
		assert!(issue.active, "failure issue is active");
		assert_eq!(
			group_issue_open_links(&mut conn, group_id, refs::MAINTENANCE_ERROR).await,
			1,
			"maintenance failure opens an incident",
		);
		// Staleness must NOT fire for a freshly-created group.
		assert!(
			group_issue(&mut conn, group_id, refs::MAINTENANCE_STALE)
				.await
				.is_none(),
			"a recent config is not maintenance-stale",
		);

		// A newer successful run is now the latest finished run → clears it.
		insert_maintenance_run(
			&mut conn,
			group_id,
			"full",
			"success",
			None,
			SignedDuration::from_secs(0),
		)
		.await;
		database::backup::staleness::sweep(&mut conn, &rows)
			.await
			.expect("re-sweep");

		let cleared = group_issue(&mut conn, group_id, refs::MAINTENANCE_ERROR)
			.await
			.expect("issue row persists");
		assert!(
			!cleared.active,
			"failure issue cleared after a successful run",
		);
		assert_eq!(
			group_issue_open_links(&mut conn, group_id, refs::MAINTENANCE_ERROR).await,
			0,
			"recovery removes the failure issue from its incident",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn in_flight_run_does_not_clear_an_open_maintenance_error() {
	TestDb::run(|mut conn, _url| async move {
		let group_id = insert_group(&mut conn, "g").await;
		insert_ready_config(&mut conn, group_id, SignedDuration::from_hours(1)).await;
		insert_maintenance_run(
			&mut conn,
			group_id,
			"full",
			"failure",
			Some("boom"),
			SignedDuration::from_hours(1),
		)
		.await;

		let rows = database::backup::staleness::scan_rows(&mut conn)
			.await
			.expect("scan");
		database::backup::staleness::sweep(&mut conn, &rows)
			.await
			.expect("sweep");
		assert!(
			group_issue(&mut conn, group_id, refs::MAINTENANCE_ERROR)
				.await
				.expect("error filed")
				.active
		);

		// A run that has started but not finished (outcome NULL) must be ignored:
		// it is not evidence that the failure recovered.
		database::BackupMaintenanceRun::start(
			&mut conn,
			group_id,
			commons_types::backup::MaintenanceKind::Full,
		)
		.await
		.expect("start in-flight run");
		database::backup::staleness::sweep(&mut conn, &rows)
			.await
			.expect("re-sweep");

		assert!(
			group_issue(&mut conn, group_id, refs::MAINTENANCE_ERROR)
				.await
				.expect("error still present")
				.active,
			"an in-flight run must not clear the failure issue",
		);
	})
	.await;
}
