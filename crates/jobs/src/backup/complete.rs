//! Completion logic: write the ground truth from a kopia op's typed outcome.
//!
//! The `backups` Deployment runs kopia in-process ([`super::kopia`]) and calls
//! these fns inline with the typed outcomes — closing the maintenance run /
//! upserting the repo inventory + stats / advancing the init status — and
//! raising or recovering the corruption alert off `verify_ok`. The inputs are
//! the typed kopia outcomes plus the group/run identifiers.

use std::str::FromStr;

use commons_types::{backup::BackupType, issue::Severity};
use database::{BackupConfigStatus, BackupMaintenanceRun, RunOutcome, ServerGroupBackupConfig};
use diesel_async::AsyncPgConnection;
use jiff::Timestamp;
use uuid::Uuid;

use super::kopia::{InspectOutcome, MaintOutcome};

/// Close a maintenance run with its outcome. On kopia success, pass the
/// [`MaintOutcome`] (bytes); on failure, pass `None` + the error message.
pub(crate) async fn complete_maint(
	db: &mut AsyncPgConnection,
	run_id: i64,
	outcome: Option<&MaintOutcome>,
	error: Option<String>,
) -> Result<(), String> {
	let (run_outcome, error, bytes_reclaimed) = match outcome {
		Some(o) => (RunOutcome::Success, None, o.bytes_reclaimed),
		None => (RunOutcome::Failure, error, None),
	};
	BackupMaintenanceRun::finish(db, run_id, run_outcome, error, bytes_reclaimed)
		.await
		.map_err(|e| e.to_string())
}

/// Advance a group's backup config after its init op finishes. On success the
/// next status depends on how the passphrase is sourced: `from_birth` repos
/// still need the operator to escrow the canopy-generated passphrase
/// (`escrow_pending`), whereas `passphrase` repos (operator-supplied) skip
/// escrow and go straight to `ready`. On failure we surface the error and leave
/// the row in `provisioning`.
pub(crate) async fn complete_init(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
	ok: bool,
	error: Option<&str>,
) -> Result<(), String> {
	use commons_types::backup::BackupRepoMode;
	if ok {
		let config = ServerGroupBackupConfig::get_required(db, group_id)
			.await
			.map_err(|e| e.to_string())?;
		let next = match config.mode {
			BackupRepoMode::FromBirth => BackupConfigStatus::EscrowPending,
			BackupRepoMode::Passphrase => BackupConfigStatus::Ready,
		};
		ServerGroupBackupConfig::set_status(db, group_id, next)
			.await
			.map_err(|e| e.to_string())?;
	} else {
		let msg = error.unwrap_or("init failed (no error reported)");
		ServerGroupBackupConfig::set_last_init_error(db, group_id, msg)
			.await
			.map_err(|e| e.to_string())?;
	}
	Ok(())
}

/// The CORRUPTION alert (severity, single-line description, active) implied by
/// an inspect result's `verify_ok`. `verify_ok:false` → a Critical, active
/// alert; `verify_ok:true` → an Info recovery (active false). Pure so the
/// outcome→decision mapping is unit-testable.
pub(crate) fn corruption_decision(verify_ok: bool) -> (Severity, Option<&'static str>, bool) {
	if verify_ok {
		(Severity::Info, None, false)
	} else {
		(
			Severity::Critical,
			Some("backup repository verify failed"),
			true,
		)
	}
}

/// Write the ground truth from a successful inspect: upsert each source's
/// inventory row + the repo stats, then raise/recover the corruption alert off
/// `verify_ok`.
pub(crate) async fn complete_inspect(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
	outcome: &InspectOutcome,
) -> Result<(), String> {
	for e in &outcome.sources {
		let server_id = e.server_id.as_deref().and_then(|s| Uuid::parse_str(s).ok());
		let type_opt = e.type_.as_deref().map(BackupType::from);
		let latest = e
			.latest_snapshot_at
			.as_deref()
			.and_then(|s| Timestamp::from_str(s).ok());
		database::backups::BackupRepoSnapshot::upsert(
			db,
			group_id,
			&e.source,
			server_id,
			type_opt.as_ref(),
			latest,
		)
		.await
		.map_err(|err| err.to_string())?;
	}

	database::backups::BackupRepoStats::upsert_repo_fields(
		db,
		group_id,
		Some(outcome.snapshot_count),
		Some(outcome.source_count),
		Some(outcome.logical_bytes),
		outcome.physical_bytes,
	)
	.await
	.map_err(|err| err.to_string())?;

	let (severity, description, active) = corruption_decision(outcome.verify_ok);
	database::backup::alerts::raise_group_event(
		db,
		group_id,
		database::backup::alerts::refs::CORRUPTION,
		severity,
		description,
		"kopia snapshot verify",
		active,
	)
	.await
	.map_err(|err| err.to_string())?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn corruption_decision_maps_verify_ok() {
		assert_eq!(
			corruption_decision(false),
			(
				Severity::Critical,
				Some("backup repository verify failed"),
				true
			)
		);
		assert_eq!(corruption_decision(true), (Severity::Info, None, false));
	}

	mod db {
		use super::super::*;
		use crate::backup::kopia::{InspectOutcome, MaintOutcome, SourceEntry};
		use commons_tests::db::TestDb;
		use commons_types::backup::{BackupRepoMode, MaintenanceKind};
		use database::diesel_async::AsyncPgConnection;
		use diesel::{sql_query, sql_types};
		use diesel_async::RunQueryDsl;

		async fn insert_group(conn: &mut AsyncPgConnection, name: &str) -> Uuid {
			#[derive(diesel::QueryableByName)]
			struct RowId {
				#[diesel(sql_type = sql_types::Uuid)]
				id: Uuid,
			}
			sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
				.bind::<sql_types::Text, _>(name)
				.get_result::<RowId>(conn)
				.await
				.expect("insert group")
				.id
		}

		async fn insert_config(conn: &mut AsyncPgConnection, group_id: Uuid) {
			ServerGroupBackupConfig::insert(
				conn,
				database::backups::NewServerGroupBackupConfig {
					group_id,
					bucket: "b".into(),
					prefix: String::new(),
					target_role_arn: "arn".into(),
					maintenance_role_arn: "maint-arn".into(),
					region: None,
					repo_password_ref: "s".into(),
					status: BackupConfigStatus::Provisioning,
					mode: BackupRepoMode::FromBirth,
				},
			)
			.await
			.expect("insert config");
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn init_success_advances_status() {
			TestDb::run(|mut conn, _url| async move {
				let group_id = insert_group(&mut conn, "g").await;
				insert_config(&mut conn, group_id).await;

				complete_init(&mut conn, group_id, true, None)
					.await
					.expect("complete_init ok");

				// from_birth → escrow_pending.
				let config = ServerGroupBackupConfig::get_required(&mut conn, group_id)
					.await
					.unwrap();
				assert_eq!(config.status, BackupConfigStatus::EscrowPending);
				assert!(config.last_init_error.is_none());
			})
			.await;
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn init_failure_records_error() {
			TestDb::run(|mut conn, _url| async move {
				let group_id = insert_group(&mut conn, "g").await;
				insert_config(&mut conn, group_id).await;

				complete_init(&mut conn, group_id, false, Some("boom"))
					.await
					.expect("complete_init err path ok");

				let config = ServerGroupBackupConfig::get_required(&mut conn, group_id)
					.await
					.unwrap();
				assert_eq!(config.status, BackupConfigStatus::Provisioning);
				assert_eq!(config.last_init_error.as_deref(), Some("boom"));
			})
			.await;
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn maint_finish_records_outcome_and_bytes() {
			TestDb::run(|mut conn, _url| async move {
				let group_id = insert_group(&mut conn, "g").await;
				let run_id =
					BackupMaintenanceRun::start(&mut conn, group_id, MaintenanceKind::Quick)
						.await
						.expect("start run");
				assert!(
					BackupMaintenanceRun::is_open(&mut conn, run_id)
						.await
						.unwrap()
				);

				let outcome = MaintOutcome {
					bytes_reclaimed: Some(4096),
				};
				complete_maint(&mut conn, run_id, Some(&outcome), None)
					.await
					.expect("complete_maint ok");

				let runs = BackupMaintenanceRun::list_for_group(&mut conn, group_id, 10)
					.await
					.unwrap();
				let run = runs.iter().find(|r| r.id == run_id).unwrap();
				assert_eq!(run.outcome, Some(RunOutcome::Success));
				assert_eq!(run.bytes_reclaimed, Some(4096));
				assert!(
					!BackupMaintenanceRun::is_open(&mut conn, run_id)
						.await
						.unwrap()
				);
			})
			.await;
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn maint_failure_records_failure() {
			TestDb::run(|mut conn, _url| async move {
				let group_id = insert_group(&mut conn, "g").await;
				let run_id =
					BackupMaintenanceRun::start(&mut conn, group_id, MaintenanceKind::Quick)
						.await
						.expect("start run");

				complete_maint(&mut conn, run_id, None, Some("kopia exploded".into()))
					.await
					.expect("complete_maint failure path ok");

				let runs = BackupMaintenanceRun::list_for_group(&mut conn, group_id, 10)
					.await
					.unwrap();
				let run = runs.iter().find(|r| r.id == run_id).unwrap();
				assert_eq!(run.outcome, Some(RunOutcome::Failure));
				assert!(run.bytes_reclaimed.is_none());
			})
			.await;
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn inspect_verify_failure_files_critical_corruption() {
			TestDb::run(|mut conn, _url| async move {
				let group_id = insert_group(&mut conn, "g").await;

				let outcome = InspectOutcome {
					verify_ok: false,
					snapshot_count: 1,
					source_count: 1,
					logical_bytes: 0,
					physical_bytes: None,
					sources: vec![SourceEntry {
						source: "canopy@srv:tamanu-postgres".into(),
						server_id: None,
						type_: Some("tamanu-postgres".into()),
						latest_snapshot_at: None,
					}],
				};
				complete_inspect(&mut conn, group_id, &outcome)
					.await
					.expect("complete_inspect ok");

				#[derive(diesel::QueryableByName)]
				struct CorruptionRow {
					#[diesel(sql_type = sql_types::Text)]
					severity: String,
					#[diesel(sql_type = sql_types::Bool)]
					active: bool,
				}
				let rows: Vec<CorruptionRow> = sql_query(
					"SELECT severity, active FROM issues \
					 WHERE server_group_id = $1 AND \"ref\" = $2",
				)
				.bind::<sql_types::Uuid, _>(group_id)
				.bind::<sql_types::Text, _>(database::backup::alerts::refs::CORRUPTION)
				.get_results(&mut conn)
				.await
				.expect("query corruption issues");
				assert_eq!(rows.len(), 1, "exactly one corruption issue");
				assert_eq!(rows[0].severity, "critical");
				assert!(rows[0].active, "corruption issue is active");
			})
			.await;
		}
	}
}
