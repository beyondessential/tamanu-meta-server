//! Inspection scheduler. Per-minute tick: for each `ready` group
//! due on its hash-jittered cadence, claim a per-group + concurrency slot and
//! **spawn a tokio task** that runs a **read-only** kopia inspection in-process
//! ([`super::kopia::run_inspect`]) and writes the ground truth inline
//! ([`super::complete::complete_inspect`]): upserts `backup_repo_snapshots` +
//! the repo fields of `backup_repo_stats`, and raises/recovers the group-level
//! `CORRUPTION` alert off `verify_ok`.
//!
//! Inspection is read-only, but it still goes through the in-flight set
//! ([`super::worker`]) so it doesn't overlap a maintenance run on the same repo.
//!
//! The per-group inspection cadence is the group's effective backup interval
//! (min across enabled types' schedule/default `expected_interval`), floored to
//! weekly.

use std::time::Duration;

use commons_servers::backup_jobs::{effective_interval_for_group, slot_is_due};
use database::{BackupConfigStatus, ServerGroupBackupConfig};
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
	let lease = worker
		.creds
		.lease(&config.maintenance_role_arn, config.region.as_deref())
		.await?;
	let env = KopiaEnv {
		creds_uri: lease.uri().to_string(),
		creds_token: lease.token().to_string(),
		region: config.region.clone(),
		password,
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
		// Per-group cadence: the group's effective backup interval, floored to
		// weekly (also weekly when no enabled type has an interval).
		let window = effective_interval_for_group(&mut db, c.group_id)
			.await
			.map_err(|e| e.to_string())?
			.map_or(INSPECT_FLOOR, |i| i.max(INSPECT_FLOOR));
		let into = secs_into(now, window);
		if !slot_is_due(c.group_id, window, TICK, into) {
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
