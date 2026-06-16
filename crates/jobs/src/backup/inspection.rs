//! Inspection scheduler (component 3). Per minute tick: reap finished inspection
//! Jobs, then spawn a **read-only** inspection Job per `ready` group due on its
//! hash-jittered cadence and not already running. The Job runs
//! `kopia snapshot list` + repo stats + verify and writes
//! `backup_repo_snapshots` / `backup_repo_stats` repo fields itself (image
//! entrypoint, ops-owned) and raises the corruption alert on a verify failure;
//! this loop only schedules + reaps.
//!
//! NOTE (flagged): the per-group inspection cadence is per the design ≈
//! `expected_interval` with a weekly floor, but that interval is now
//! per-`(group, type)` (backup-types addendum); this first cut uses a daily
//! default. The read-only inspection SA + Deployment are ops-owned.

use std::time::Duration;

use crate::backup::{
	jobspec::{JobParams, build_job},
	spawn::{JobSpawner, KubeSpawner},
};
use commons_servers::backup_jobs::{BillingLabels, JobKind, slot_is_due};
use database::{BackupConfigStatus, ServerGroupBackupConfig, server_groups::ServerGroup};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info, warn};

const TICK: Duration = Duration::from_secs(60);
const INSPECT_WINDOW: Duration = Duration::from_secs(24 * 3600);

struct Cfg {
	namespace: String,
	image: String,
	inspection_sa: String,
	password_key: String,
}

impl Cfg {
	fn from_env() -> Self {
		Cfg {
			namespace: env_or("CANOPY_NAMESPACE", "tamanu-meta"),
			image: env_or("CANOPY_BACKUP_IMAGE", "kopia-job:latest"),
			inspection_sa: env_or("CANOPY_BACKUP_INSPECTION_SA", "canopy-inspection"),
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

async fn reap_finished(spawner: &KubeSpawner) {
	match spawner.finished_jobs().await {
		Ok(finished) => {
			for job in finished {
				// Inspection Jobs carry no run id; just reap the finished ones.
				if job.kind.as_deref() == Some("inspect")
					&& let Err(e) = spawner.delete_job(&job.name).await
				{
					warn!("inspection: deleting finished job {} failed: {e}", job.name);
				}
			}
		}
		Err(e) => warn!("inspection: listing finished jobs failed: {e}"),
	}
}

async fn tick(
	db: &mut diesel_async::AsyncPgConnection,
	spawner: &KubeSpawner,
	cfg: &Cfg,
) -> Result<(), String> {
	reap_finished(spawner).await;

	let ready: Vec<ServerGroupBackupConfig> = ServerGroupBackupConfig::list(db)
		.await
		.map_err(|e| e.to_string())?
		.into_iter()
		.filter(|c| c.status == BackupConfigStatus::Ready)
		.collect();
	let active = spawner.active_groups().await?;
	let now = Timestamp::now();
	let into = secs_into(now, INSPECT_WINDOW);

	for c in &ready {
		if active.contains(&c.group_id) {
			continue;
		}
		if !slot_is_due(c.group_id, INSPECT_WINDOW, TICK, into) {
			continue;
		}
		let highest = ServerGroup::highest_member_ranks(db, &[c.group_id])
			.await
			.map_err(|e| e.to_string())?
			.remove(&c.group_id);
		let group = ServerGroup::get_by_id(db, c.group_id)
			.await
			.map_err(|e| e.to_string())?;
		let billing = BillingLabels::from_group(&group.tags, &group.name, highest);

		let job = build_job(&JobParams {
			namespace: cfg.namespace.clone(),
			kind: JobKind::Inspect,
			group_id: c.group_id,
			image: cfg.image.clone(),
			service_account: cfg.inspection_sa.clone(),
			bucket: c.bucket.clone(),
			prefix: c.prefix.clone(),
			region: c.region.clone(),
			target_role_arn: c.target_role_arn.clone(),
			retention_json: String::new(), // inspection is read-only; no retention arg
			repo_password_secret: c.repo_password_ref.clone(),
			repo_password_key: cfg.password_key.clone(),
			billing,
			run_id: None,
		});
		match spawner.spawn(job).await {
			Ok(name) => info!(group = %c.group_id, "spawned inspection job {name}"),
			Err(e) => error!(group = %c.group_id, "spawning inspection job failed: {e}"),
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
						error!("inspection: kube client init failed (will retry): {e}");
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
				error!("inspection tick failed: {e}");
			} else {
				debug!("inspection tick ok");
			}
		}
	})
}
