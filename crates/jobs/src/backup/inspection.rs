//! Inspection scheduler. Per-minute tick: for each `ready` group that's due,
//! claim a per-group + concurrency slot and **spawn a tokio task** that runs a
//! **read-only** kopia inspection in-process ([`super::kopia::run_inspect`]) and
//! writes the ground truth inline ([`super::complete::complete_inspect`]):
//! upserts `backup_repo_snapshots` + the repo fields of `backup_repo_stats`, and
//! raises/recovers the group-level `CORRUPTION` alert off `verify_ok`.
//!
//! Inspection is read-only, but it still goes through the in-flight set
//! ([`super::worker`]) so it doesn't overlap a maintenance run on the same repo.
//!
//! A group is due when any of these hold:
//!
//! - **a backup has landed since its last inspection** — so the stats panel
//!   freshens shortly after a backup (including a manual "back up now")
//!   rather than waiting;
//! - **a maintenance run has completed successfully since its last
//!   inspection** — maintenance prunes/compacts the repo, so the inventory
//!   should freshen after it too;
//! - otherwise, its **hash-jittered cadence** has come around: the group's
//!   effective backup interval (min across enabled types' schedule/default
//!   `expected_interval`), floored to weekly, so corruption/drift is still
//!   caught on quiet repos.

use std::time::Duration;

use commons_servers::backup_jobs::{effective_interval_for_group, slot_is_due};
use database::{
	BackupConfigStatus, BackupMaintenanceRun, BackupRepoSnapshot, BackupRun,
	ServerGroupBackupConfig,
};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info};

use super::{
	complete,
	kopia::{self, KopiaEnv},
	worker::Worker,
};

const TICK: Duration = Duration::from_secs(60);
/// Floor for the per-group inspection cadence: inspect at least weekly even when
/// a group's effective backup interval is longer (or absent).
const INSPECT_FLOOR: Duration = Duration::from_secs(7 * 24 * 3600);

fn secs_into(now: Timestamp, window: Duration) -> u64 {
	let w = window.as_secs().max(1) as i64;
	now.as_second().rem_euclid(w) as u64
}

/// Whether a backup has landed since the repo was last inspected — the signal to
/// inspect promptly (so the stats panel freshens shortly after a backup,
/// including a manual "back up now") instead of waiting for the weekly cadence.
/// `latest_backup` is the group's newest successful backup; `last_inspected` is
/// the newest inspection. Never-inspected with a backup present → due.
fn backup_since_inspect(
	latest_backup: Option<Timestamp>,
	last_inspected: Option<Timestamp>,
) -> bool {
	match (latest_backup, last_inspected) {
		(Some(backup), Some(inspected)) => backup > inspected,
		(Some(_), None) => true,
		(None, _) => false,
	}
}

/// Whether a maintenance run has completed successfully since the repo was
/// last inspected — the signal to inspect promptly after maintenance (pruning
/// and compaction change what's actually stored, so the inventory should
/// freshen). `latest_maint` is the group's newest successful maintenance
/// completion; `last_inspected` is the newest inspection. Never-inspected with
/// a completed maintenance run present → due. A failed run doesn't count: it
/// hasn't changed repo contents, and it's already surfaced via the separate
/// maintenance-error alert.
fn maintenance_since_inspect(
	latest_maint: Option<Timestamp>,
	last_inspected: Option<Timestamp>,
) -> bool {
	match (latest_maint, last_inspected) {
		(Some(maint), Some(inspected)) => maint > inspected,
		(Some(_), None) => true,
		(None, _) => false,
	}
}

/// Run an inspection op in a spawned task: read the password, run
/// `kopia::run_inspect`, then write the ground truth + corruption alert.
fn spawn_inspect(worker: &Worker, config: ServerGroupBackupConfig) {
	let Some(guard) = worker.try_claim(config.group_id) else {
		return;
	};
	let worker = worker.clone();
	task::spawn(async move {
		let _guard = guard;
		let group_id = config.group_id;
		let result = run_inspect_op(&worker, &config).await;
		match result {
			Ok(outcome) => {
				let Ok(mut db) = worker.pool.get().await else {
					error!(group = %group_id, "inspection: failed to get db connection to record result");
					return;
				};
				if let Err(e) = complete::complete_inspect(&mut db, group_id, &outcome).await {
					error!(group = %group_id, "inspection: recording result failed: {e}");
				} else {
					info!(group = %group_id, "inspection complete (verify_ok={})", outcome.verify_ok);
				}
			}
			Err(e) => {
				// A hard inspect failure means we don't know the verify state, so we
				// don't write ground truth or touch the corruption alert — leave the
				// last known truth in place.
				error!(group = %group_id, "inspection failed: {e:#}");
			}
		}
	});
}

/// The kopia side of an inspection op.
async fn run_inspect_op(
	worker: &Worker,
	config: &ServerGroupBackupConfig,
) -> anyhow::Result<kopia::InspectOutcome> {
	let password = worker.read_repo_password(&config.repo_password_ref).await?;
	let creds = worker
		.creds
		.resolve(&config.maintenance_role_arn, config.region.as_deref())
		.await?;
	let env = KopiaEnv {
		access_key_id: creds.access_key_id,
		secret_access_key: creds.secret_access_key,
		session_token: creds.session_token,
		region: config.region.clone(),
		password,
		proxy_endpoint: None,
	};
	let region = config.region.as_deref().unwrap_or_default();
	kopia::run_inspect(&env, &config.bucket, &config.prefix, region).await
}

async fn tick(worker: &Worker) -> Result<(), String> {
	let mut db = worker.pool.get().await.map_err(|e| e.to_string())?;
	let ready: Vec<ServerGroupBackupConfig> = ServerGroupBackupConfig::list(&mut db)
		.await
		.map_err(|e| e.to_string())?
		.into_iter()
		.filter(|c| c.status == BackupConfigStatus::Ready)
		.collect();
	let in_flight = worker.in_flight_snapshot();
	let now = Timestamp::now();

	for c in &ready {
		if in_flight.contains(&c.group_id) {
			continue; // already mid-op
		}
		// Inspect promptly once a backup has landed since the last inspection
		// (so the panel freshens shortly after a backup / "back up now")...
		let latest_backup = BackupRun::latest_backup_at_for_group(&mut db, c.group_id)
			.await
			.map_err(|e| e.to_string())?;
		let last_inspected = BackupRepoSnapshot::last_inspected_at_for_group(&mut db, c.group_id)
			.await
			.map_err(|e| e.to_string())?;
		let due_after_backup = backup_since_inspect(latest_backup, last_inspected);

		// ...or a maintenance run has completed successfully since the last
		// inspection (pruning/compaction changes the repo, so freshen the
		// inventory the same way a landed backup does)...
		let latest_maint =
			BackupMaintenanceRun::latest_successful_finished_at_for_group(&mut db, c.group_id)
				.await
				.map_err(|e| e.to_string())?;
		let due_after_maint = maintenance_since_inspect(latest_maint, last_inspected);

		// ...otherwise fall back to the per-group cadence: the group's effective
		// backup interval, floored to weekly (also the floor when no enabled type
		// has an interval), so corruption/drift is still caught on quiet repos.
		let window = effective_interval_for_group(&mut db, c.group_id)
			.await
			.map_err(|e| e.to_string())?
			.map_or(INSPECT_FLOOR, |i| i.max(INSPECT_FLOOR));
		let into = secs_into(now, window);
		if !due_after_backup && !due_after_maint && !slot_is_due(c.group_id, window, TICK, into) {
			continue;
		}
		spawn_inspect(worker, c.clone());
	}
	Ok(())
}

pub fn spawn(worker: Worker) -> JoinHandle<()> {
	task::spawn(async move {
		loop {
			sleep(TICK).await;
			if let Err(e) = tick(&worker).await {
				error!("inspection tick failed: {e}");
			} else {
				debug!("inspection tick ok");
			}
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn backup_since_inspect_decisions() {
		let t0: Timestamp = "2026-06-01T00:00:00Z".parse().unwrap();
		let t1: Timestamp = "2026-06-01T01:00:00Z".parse().unwrap();
		// Backup newer than last inspection → due.
		assert!(backup_since_inspect(Some(t1), Some(t0)));
		// Inspection at/after the latest backup → not due.
		assert!(!backup_since_inspect(Some(t0), Some(t1)));
		assert!(!backup_since_inspect(Some(t0), Some(t0)));
		// Backup present but never inspected → due.
		assert!(backup_since_inspect(Some(t0), None));
		// No successful backup yet → nothing to inspect for.
		assert!(!backup_since_inspect(None, None));
		assert!(!backup_since_inspect(None, Some(t0)));
	}

	#[test]
	fn maintenance_since_inspect_decisions() {
		let t0: Timestamp = "2026-06-01T00:00:00Z".parse().unwrap();
		let t1: Timestamp = "2026-06-01T01:00:00Z".parse().unwrap();
		// Maintenance completed newer than last inspection → due.
		assert!(maintenance_since_inspect(Some(t1), Some(t0)));
		// Inspection at/after the latest maintenance completion → not due.
		assert!(!maintenance_since_inspect(Some(t0), Some(t1)));
		assert!(!maintenance_since_inspect(Some(t0), Some(t0)));
		// Maintenance completed but never inspected → due.
		assert!(maintenance_since_inspect(Some(t0), None));
		// No successful maintenance run yet → nothing to inspect for.
		assert!(!maintenance_since_inspect(None, None));
		assert!(!maintenance_since_inspect(None, Some(t0)));
	}
}
