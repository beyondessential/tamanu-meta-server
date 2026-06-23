//! Maintenance scheduler. Per-minute tick: list configs, and for each
//! `provisioning` group needing init and each `ready` group due on its
//! hash-jittered cadence (quick-daily / full-weekly; full subsumes quick),
//! claim a per-group + concurrency slot and **spawn a tokio task** that runs
//! kopia in-process ([`super::kopia`]) and writes the result inline
//! ([`super::complete`]).
//!
//! The in-flight group set ([`super::worker`]) ensures one op per group at a
//! time across maintenance + inspection + init, and the semaphore caps total
//! concurrency.
//!
//! Retention is resolved per-`(group, type)` (schedule override → type default →
//! org floor) and applied per source by the kopia layer. The kopia subprocess
//! assumes the group's per-bucket role via web-identity directly (refreshing),
//! and reads `KOPIA_PASSWORD` from the group's k8s Secret.

use std::{collections::HashSet, time::Duration};

use commons_servers::backup_jobs::{
	JobKind, SharedBackupConfig, backup_bucket_billing_tags, effective_retention_for_group, is_due,
	slot_is_due,
};
use commons_types::backup::BackupPlacement;
use database::{
	BackupConfigStatus, BackupMaintenanceRun, BackupRepoMode, MaintenanceKind, RunOutcome,
	ServerGroupBackupConfig, server_groups::ServerGroup,
};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info};

use super::{
	complete,
	creds_server::CredsLease,
	kopia::{self, KopiaEnv, RetentionMap},
	worker::Worker,
};

const TICK: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(24 * 3600);
const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);

fn secs_into(now: Timestamp, window: Duration) -> u64 {
	let w = window.as_secs().max(1) as i64;
	now.as_second().rem_euclid(w) as u64
}

/// Build the `{type → policy}` retention map for a group: the effective
/// `RetentionPolicy` per enabled backup type, serialised through JSON into the
/// kopia layer's [`RetentionMap`]. Empty when the group has no enabled types
/// (the kopia layer falls back to the org floor for the global baseline).
async fn retention_map_for_group(
	db: &mut diesel_async::AsyncPgConnection,
	group_id: uuid::Uuid,
) -> Result<RetentionMap, String> {
	let pairs = effective_retention_for_group(db, group_id)
		.await
		.map_err(|e| e.to_string())?;
	let mut map = RetentionMap::new();
	for (ty, policy) in pairs {
		let value = serde_json::to_value(policy).map_err(|e| e.to_string())?;
		let policy: kopia::Policy = serde_json::from_value(value).map_err(|e| e.to_string())?;
		map.insert(ty.as_str().to_string(), policy);
	}
	Ok(map)
}

/// Build the per-op kopia env from a creds lease, the group's region, and its
/// repo password. The `lease` must outlive every kopia invocation in the op.
fn kopia_env(lease: &CredsLease, config: &ServerGroupBackupConfig, password: String) -> KopiaEnv {
	KopiaEnv {
		creds_uri: lease.uri().to_string(),
		creds_token: lease.token().to_string(),
		region: config.region.clone(),
		password,
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

/// Whether a config needs its init (repo-create) op run this tick. True iff the
/// row is freshly `provisioning` with no recorded init error and no op already
/// in-flight for the group. A failed attempt sets `last_init_error`, so we don't
/// retry until the operator clears it via `mark_provisioning` — this prevents an
/// infinite per-tick retry loop.
fn needs_init(config: &ServerGroupBackupConfig, in_flight: &HashSet<uuid::Uuid>) -> bool {
	config.status == BackupConfigStatus::Provisioning
		&& config.last_init_error.is_none()
		&& !in_flight.contains(&config.group_id)
}

/// Run an init op in a spawned task: read the password, resolve retention, run
/// `kopia::run_init`, then advance the config (or record the error). The guard
/// drops on completion, releasing the group + permit.
fn spawn_init(worker: &Worker, config: ServerGroupBackupConfig) {
	let Some(guard) = worker.try_claim(config.group_id) else {
		return;
	};
	let worker = worker.clone();
	task::spawn(async move {
		let _guard = guard;
		let group_id = config.group_id;
		let result = run_init_op(&worker, &config).await;

		let Ok(mut db) = worker.pool.get().await else {
			error!(group = %group_id, "init: failed to get db connection to record result");
			return;
		};
		match result {
			Ok(()) => {
				if let Err(e) = complete::complete_init(&mut db, group_id, true, None).await {
					error!(group = %group_id, "init: recording success failed: {e}");
				} else {
					info!(group = %group_id, "init complete");
				}
			}
			Err(e) => {
				let msg = format!("{e:#}");
				error!(group = %group_id, "init failed: {msg}");
				if let Err(e) = complete::complete_init(&mut db, group_id, false, Some(&msg)).await
				{
					error!(group = %group_id, "init: recording failure failed: {e}");
				}
			}
		}
	});
}

/// The kopia side of an init op: read the password + retention, run init.
async fn run_init_op(worker: &Worker, config: &ServerGroupBackupConfig) -> anyhow::Result<()> {
	// Shared-account configs: Canopy owns the bucket. Stamp the shared role ARNs +
	// region into the row (from the backups pod's env) and create + configure the
	// bucket — all before kopia connects. BYO (`external`) rows already carry their
	// ARNs, so this is a no-op for them.
	let config = if config.placement == BackupPlacement::Shared {
		provision_shared(worker, config).await?
	} else {
		config.clone()
	};
	let config = &config;

	let password = worker.read_repo_password(&config.repo_password_ref).await?;
	let lease = worker
		.creds
		.lease(&config.maintenance_role_arn, config.region.as_deref())
		.await?;
	let env = kopia_env(&lease, config, password);
	let retention = {
		let mut db = worker
			.pool
			.get()
			.await
			.map_err(|e| anyhow::anyhow!("db connection: {e}"))?;
		retention_map_for_group(&mut db, config.group_id)
			.await
			.map_err(|e| anyhow::anyhow!(e))?
	};
	let region = config.region.as_deref().unwrap_or_default();
	// from-birth creates the repo; passphrase mode connects to an existing one
	// only (Canopy never creates a repo under an operator-chosen passphrase).
	let create_new = matches!(config.mode, BackupRepoMode::FromBirth);
	kopia::run_init(
		&env,
		&config.bucket,
		&config.prefix,
		region,
		&retention,
		create_new,
	)
	.await?;

	// Ensure the bucket's Intelligent-Tiering .storageconfig exists (pulumi
	// normally writes it; create as a fallback, never overwrite). Best-effort —
	// it only affects S3 tiering, not backup correctness.
	if let Err(e) = super::storageconfig::ensure(
		&config.maintenance_role_arn,
		&config.bucket,
		&config.prefix,
		config.region.as_deref(),
	)
	.await
	{
		tracing::warn!(group = %config.group_id, ".storageconfig ensure failed (non-fatal): {e:#}");
	}

	// Import: Canopy never persists the operator's passphrase — immediately
	// rotate it to a generated one (a Canopy import is a hard break for any
	// existing consumers, which is intended).
	if matches!(config.mode, BackupRepoMode::Passphrase) {
		super::rotation::rotate(worker, config).await.map_err(|e| {
			anyhow::anyhow!("import: rotating to a canopy-generated passphrase: {e}")
		})?;
	}
	Ok(())
}

/// Provision a `placement=shared` config: resolve the shared-account settings
/// from the backups pod's `CANOPY_SHARED_BACKUP_*` env, **stamp the device /
/// maintenance role ARNs + region into the row** (so public-server's device-cred
/// path and the maintenance lease read them like any other group), then create +
/// configure the bucket (idempotent — see [`super::provision`]). Returns the
/// updated config. A missing/incomplete env is a clear error, recorded as
/// `last_init_error` — the requirement lives here, on the pod that has the env,
/// not on private-server's onboarding endpoint.
async fn provision_shared(
	worker: &Worker,
	config: &ServerGroupBackupConfig,
) -> anyhow::Result<ServerGroupBackupConfig> {
	let shared = SharedBackupConfig::from_env()
		.filter(|s| !s.provisioner_role_arn.trim().is_empty())
		.ok_or_else(|| {
			anyhow::anyhow!(
				"shared-account backups are not configured: set CANOPY_SHARED_BACKUP_REGION, \
				 _DEVICE_ROLE_ARN, _MAINTENANCE_ROLE_ARN and _PROVISIONER_ROLE_ARN on the backups pod"
			)
		})?;
	// An operator-supplied region wins; otherwise the shared-account default.
	let region = config
		.region
		.clone()
		.unwrap_or_else(|| shared.region.clone());

	let mut db = worker
		.pool
		.get()
		.await
		.map_err(|e| anyhow::anyhow!("db connection: {e}"))?;
	let config = ServerGroupBackupConfig::update_roles_region(
		&mut db,
		config.group_id,
		&shared.device_role_arn,
		&shared.maintenance_role_arn,
		Some(&region),
	)
	.await
	.map_err(|e| anyhow::anyhow!(e))?;

	let group = ServerGroup::get_by_id(&mut db, config.group_id)
		.await
		.map_err(|e| anyhow::anyhow!(e))?;
	let highest_rank = ServerGroup::highest_member_ranks(&mut db, &[config.group_id])
		.await
		.map_err(|e| anyhow::anyhow!(e))?
		.get(&config.group_id)
		.copied();
	let tags = backup_bucket_billing_tags(&group.tags, &group.name, highest_rank);

	super::provision::ensure_bucket(&shared.provisioner_role_arn, &config.bucket, &region, &tags)
		.await?;
	Ok(config)
}

/// Run a maintenance op in a spawned task: start the run row, read the password,
/// resolve retention, run `kopia::run_maintenance`, then close the run row.
fn spawn_maint(worker: &Worker, config: ServerGroupBackupConfig, kind: MaintenanceKind) {
	let Some(guard) = worker.try_claim(config.group_id) else {
		return;
	};
	let worker = worker.clone();
	task::spawn(async move {
		let _guard = guard;
		let group_id = config.group_id;

		// Open the run row first so a crash leaves it visibly open.
		let run_id = {
			let Ok(mut db) = worker.pool.get().await else {
				error!(group = %group_id, "maintenance: failed to get db connection");
				return;
			};
			match BackupMaintenanceRun::start(&mut db, group_id, kind).await {
				Ok(id) => id,
				Err(e) => {
					error!(group = %group_id, "maintenance: starting run failed: {e}");
					return;
				}
			}
		};

		let result = run_maint_op(&worker, &config, kind).await;

		let Ok(mut db) = worker.pool.get().await else {
			error!(group = %group_id, run_id, "maintenance: failed to get db connection to record result");
			return;
		};
		let recorded = match &result {
			Ok(outcome) => complete::complete_maint(&mut db, run_id, Some(outcome), None).await,
			Err(e) => {
				let msg = format!("{e:#}");
				error!(group = %group_id, run_id, "maintenance failed: {msg}");
				complete::complete_maint(&mut db, run_id, None, Some(msg)).await
			}
		};
		match recorded {
			Ok(()) if result.is_ok() => {
				info!(group = %group_id, kind = ?kind, run_id, "maintenance complete")
			}
			Ok(()) => {}
			Err(e) => {
				error!(group = %group_id, run_id, "maintenance: recording result failed: {e}")
			}
		}
	});
}

/// The kopia side of a maintenance op.
async fn run_maint_op(
	worker: &Worker,
	config: &ServerGroupBackupConfig,
	kind: MaintenanceKind,
) -> anyhow::Result<kopia::MaintOutcome> {
	let password = worker.read_repo_password(&config.repo_password_ref).await?;
	let lease = worker
		.creds
		.lease(&config.maintenance_role_arn, config.region.as_deref())
		.await?;
	let env = kopia_env(&lease, config, password);
	let retention = {
		let mut db = worker
			.pool
			.get()
			.await
			.map_err(|e| anyhow::anyhow!("db connection: {e}"))?;
		retention_map_for_group(&mut db, config.group_id)
			.await
			.map_err(|e| anyhow::anyhow!(e))?
	};
	let region = config.region.as_deref().unwrap_or_default();
	kopia::run_maintenance(
		&env,
		&config.bucket,
		&config.prefix,
		region,
		kind,
		&retention,
	)
	.await
}

async fn tick(worker: &Worker) -> Result<(), String> {
	let mut db = worker.pool.get().await.map_err(|e| e.to_string())?;
	let all: Vec<ServerGroupBackupConfig> = ServerGroupBackupConfig::list(&mut db)
		.await
		.map_err(|e| e.to_string())?;
	let now = Timestamp::now();

	// Snapshot the in-flight set once for the cheap skip checks; `try_claim`
	// re-checks atomically when we actually spawn.
	let in_flight = worker.in_flight_snapshot();

	// Init pass: create the kopia repo for freshly-provisioned groups.
	for c in &all {
		if !needs_init(c, &in_flight) {
			continue;
		}
		spawn_init(worker, c.clone());
	}

	// Maintenance pass: ready groups due on their jittered cadence.
	let ready: Vec<&ServerGroupBackupConfig> = all
		.iter()
		.filter(|c| c.status == BackupConfigStatus::Ready)
		.collect();
	for c in &ready {
		if in_flight.contains(&c.group_id) {
			continue; // already mid-op
		}
		let runs = BackupMaintenanceRun::list_for_group(&mut db, c.group_id, 20)
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
		spawn_maint(worker, (*c).clone(), maint_kind);
	}
	Ok(())
}

pub fn spawn(worker: Worker) -> JoinHandle<()> {
	task::spawn(async move {
		loop {
			sleep(TICK).await;
			if let Err(e) = tick(&worker).await {
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

	fn config(
		group_id: uuid::Uuid,
		status: BackupConfigStatus,
		last_init_error: Option<&str>,
	) -> ServerGroupBackupConfig {
		let now = Timestamp::now();
		ServerGroupBackupConfig {
			group_id,
			bucket: "b".into(),
			prefix: String::new(),
			target_role_arn: "arn".into(),
			maintenance_role_arn: "maint-arn".into(),
			region: None,
			repo_password_ref: "s".into(),
			status,
			created_at: now,
			updated_at: now,
			mode: commons_types::backup::BackupRepoMode::FromBirth,
			last_init_error: last_init_error.map(str::to_string),
			placement: commons_types::backup::BackupPlacement::External,
		}
	}

	#[test]
	fn init_selection_logic() {
		let g = uuid::Uuid::from_u128(7);
		let empty = HashSet::new();
		let busy = HashSet::from([g]);

		// Fresh provisioning, no error, not in-flight → run init.
		assert!(needs_init(
			&config(g, BackupConfigStatus::Provisioning, None),
			&empty
		));
		// Provisioning but a prior attempt failed → wait for operator retry.
		assert!(!needs_init(
			&config(g, BackupConfigStatus::Provisioning, Some("boom")),
			&empty
		));
		// Provisioning but an op is already in-flight for the group → don't
		// double-run.
		assert!(!needs_init(
			&config(g, BackupConfigStatus::Provisioning, None),
			&busy
		));
		// Already past provisioning → not an init candidate.
		assert!(!needs_init(
			&config(g, BackupConfigStatus::Ready, None),
			&empty
		));
	}

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
