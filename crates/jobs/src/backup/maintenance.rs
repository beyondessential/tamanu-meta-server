//! Maintenance scheduler (component 3). Per minute tick: poll finished Jobs and
//! close their `backup_maintenance_runs` rows; then, for each `ready` group due
//! on its hash-jittered cadence (quick-daily / full-weekly; full subsumes
//! quick) and not already running, record a run and spawn the kopia maintenance
//! Job. The three-step cycle (assert-retention → snapshot expire → maintenance
//! run) executes inside the Job; this loop only schedules + records.
//!
//! Net-new kube/AWS infra: the kube client is built at startup and rebuilt in
//! the loop on failure (so an API-server blip doesn't kill the pod), mirroring
//! how `reachability` tolerates a missing tailnet directory.
//!
//! NOTE (flagged): retention is per-`(group, type)` under the backup-types
//! addendum; this first cut asserts the org-floor default repo-wide. Resolving
//! each active type's effective policy is the follow-up. The ops-side
//! single-replica Deployment + IRSA ServiceAccounts are owned by the ops spec.

use std::time::Duration;

use crate::backup::{
	jobspec::{JobParams, build_job},
	spawn::{JobSpawner, KubeSpawner},
};
use commons_servers::backup_jobs::{BillingLabels, JobKind, RetentionPolicy, is_due, slot_is_due};
use database::{
	BackupConfigStatus, BackupMaintenanceRun, MaintenanceKind, RunOutcome, ServerGroupBackupConfig,
	server_groups::ServerGroup,
};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info, warn};

const TICK: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(24 * 3600);
const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);

/// Scheduler config read from the environment (like DATABASE_URL), so one
/// binary works across stacks.
struct Cfg {
	namespace: String,
	image: String,
	maintenance_sa: String,
	password_key: String,
}

impl Cfg {
	fn from_env() -> Self {
		Cfg {
			namespace: env_or("CANOPY_NAMESPACE", "tamanu-meta"),
			image: env_or("CANOPY_BACKUP_IMAGE", "kopia-job:latest"),
			maintenance_sa: env_or("CANOPY_BACKUP_MAINTENANCE_SA", "canopy-maintenance"),
			password_key: env_or("CANOPY_BACKUP_PASSWORD_KEY", "password"),
		}
	}
}

fn env_or(key: &str, default: &str) -> String {
	std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn secs_into(now: Timestamp, window: Duration) -> u64 {
	let w = window.as_secs().max(1) as i64;
	now.as_second().rem_euclid(w) as u64
}

/// Close out any finished maintenance Jobs, then delete them.
async fn reconcile_finished(db: &mut diesel_async::AsyncPgConnection, spawner: &KubeSpawner) {
	let finished = match spawner.finished_jobs().await {
		Ok(f) => f,
		Err(e) => {
			warn!("maintenance: listing finished jobs failed: {e}");
			return;
		}
	};
	for job in finished {
		// Only maintenance Jobs carry a run id to close.
		if let Some(run_id) = job.run_id {
			let outcome = if job.succeeded {
				RunOutcome::Success
			} else {
				RunOutcome::Failure
			};
			if let Err(e) = BackupMaintenanceRun::finish(db, run_id, outcome, None, None).await {
				error!("maintenance: finishing run {run_id} failed: {e}");
				continue;
			}
		}
		if let Err(e) = spawner.delete_job(&job.name).await {
			warn!(
				"maintenance: deleting finished job {} failed: {e}",
				job.name
			);
		}
	}
}

/// Decide which maintenance kind (if any) a group is due for this tick.
fn due_kind(
	group_id: uuid::Uuid,
	last_quick_or_full: Option<Timestamp>,
	last_full: Option<Timestamp>,
	now: Timestamp,
) -> Option<JobKind> {
	let full_due =
		is_due(WEEK, last_full, now) && slot_is_due(group_id, WEEK, TICK, secs_into(now, WEEK));
	if full_due {
		return Some(JobKind::MaintFull);
	}
	let quick_due = is_due(DAY, last_quick_or_full, now)
		&& slot_is_due(group_id, DAY, TICK, secs_into(now, DAY));
	quick_due.then_some(JobKind::MaintQuick)
}

async fn tick(
	db: &mut diesel_async::AsyncPgConnection,
	spawner: &KubeSpawner,
	cfg: &Cfg,
) -> Result<(), String> {
	reconcile_finished(db, spawner).await;

	let ready: Vec<ServerGroupBackupConfig> = ServerGroupBackupConfig::list(db)
		.await
		.map_err(|e| e.to_string())?
		.into_iter()
		.filter(|c| c.status == BackupConfigStatus::Ready)
		.collect();
	let active = spawner.active_groups().await?;
	let now = Timestamp::now();

	for c in &ready {
		if active.contains(&c.group_id) {
			continue; // already mid-run
		}
		let runs = BackupMaintenanceRun::list_for_group(db, c.group_id, 20)
			.await
			.map_err(|e| e.to_string())?;
		let succeeded = |k: MaintenanceKind| {
			runs.iter()
				.find(|r| r.kind == k && r.outcome == Some(RunOutcome::Success))
				.map(|r| r.started_at)
		};
		let last_full = succeeded(MaintenanceKind::Full);
		// A full run subsumes quick, so quick's "last" is the newest of either.
		let last_quick = [succeeded(MaintenanceKind::Quick), last_full]
			.into_iter()
			.flatten()
			.max();

		let Some(job_kind) = due_kind(c.group_id, last_quick, last_full, now) else {
			continue;
		};
		let maint_kind = match job_kind {
			JobKind::MaintFull => MaintenanceKind::Full,
			_ => MaintenanceKind::Quick,
		};

		let run_id = BackupMaintenanceRun::start(db, c.group_id, maint_kind)
			.await
			.map_err(|e| e.to_string())?;
		let highest = ServerGroup::highest_member_ranks(db, &[c.group_id])
			.await
			.map_err(|e| e.to_string())?
			.remove(&c.group_id);
		let group = ServerGroup::get_by_id(db, c.group_id)
			.await
			.map_err(|e| e.to_string())?;
		let billing = BillingLabels::from_group(&group.tags, &group.name, highest);
		let retention_json = serde_json::to_string(
			&RetentionPolicy {
				keep_latest: 1,
				keep_daily: 0,
				keep_weekly: 0,
				keep_monthly: 0,
				keep_annual: 0,
			}
			.enforce_floor(),
		)
		.unwrap_or_else(|_| "{}".to_string());

		let job = build_job(&JobParams {
			namespace: cfg.namespace.clone(),
			kind: job_kind,
			group_id: c.group_id,
			image: cfg.image.clone(),
			service_account: cfg.maintenance_sa.clone(),
			bucket: c.bucket.clone(),
			prefix: c.prefix.clone(),
			region: c.region.clone(),
			target_role_arn: c.target_role_arn.clone(),
			retention_json,
			repo_password_secret: c.repo_password_ref.clone(),
			repo_password_key: cfg.password_key.clone(),
			billing,
			run_id: Some(run_id),
		});
		match spawner.spawn(job).await {
			Ok(name) => {
				info!(group = %c.group_id, kind = job_kind.as_str(), run_id, "spawned maintenance job {name}")
			}
			Err(e) => error!(group = %c.group_id, "spawning maintenance job failed: {e}"),
		}
	}
	Ok(())
}

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	let cfg = Cfg::from_env();
	task::spawn(async move {
		let mut spawner: Option<KubeSpawner> = None;
		loop {
			sleep(TICK).await;
			if spawner.is_none() {
				match kube::Client::try_default().await {
					Ok(client) => spawner = Some(KubeSpawner::new(client, &cfg.namespace)),
					Err(e) => {
						error!("maintenance: kube client init failed (will retry): {e}");
						continue;
					}
				}
			}
			let Some(spawner) = &spawner else { continue };
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};
			if let Err(e) = tick(&mut db, spawner, &cfg).await {
				error!("maintenance tick failed: {e}");
			} else {
				debug!("maintenance tick ok");
			}
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn maintenance_due_logic() {
		let g = uuid::Uuid::from_u128(3);
		// Recent full + quick → nothing due (elapsed gate), independent of slot.
		let now: Timestamp = "2026-06-16T12:00:00Z".parse().unwrap();
		let recent: Timestamp = "2026-06-16T11:00:00Z".parse().unwrap();
		assert_eq!(due_kind(g, Some(recent), Some(recent), now), None);

		// At the group's weekly slot with no prior full run, full is due (and
		// subsumes quick).
		let week_slot = commons_servers::backup_jobs::jitter_slot(g, WEEK).as_secs() as i64;
		let at_slot = Timestamp::from_second(week_slot).unwrap();
		assert_eq!(due_kind(g, None, None, at_slot), Some(JobKind::MaintFull));
	}
}
