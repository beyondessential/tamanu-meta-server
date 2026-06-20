//! Passphrase rotation — dual-key + crash-safe.
//!
//! The repo-password Secret carries two keys: `password` (current/committed,
//! what every reader uses) and `password_next` (the candidate during an
//! in-flight rotation). [`rotate`] persists the candidate *before* touching the
//! repo, runs `kopia change-password` (which verifies by reconnecting), then
//! promotes the candidate. [`reconcile`] finishes or abandons a rotation that
//! crashed between those steps, by probing which passphrase the repo actually
//! accepts.
//!
//! The kopia calls need a real binary, so they aren't unit-tested here; the
//! risky bits that are — the dual-key Secret transitions (tested in
//! `commons_servers::backup_secrets`) and the recovery decision
//! ([`reconcile_decision`]) — are.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Result, anyhow, bail};
use commons_servers::{backup_jobs::slot_is_due, backup_secrets::generate_passphrase};
use database::{BackupConfigStatus, ServerGroupBackupConfig};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info, warn};

use super::{
	creds_server::CredsLease,
	kopia::{self, KopiaEnv},
	worker::Worker,
};

/// Committed passphrase key.
const KEY_CURRENT: &str = "password";
/// In-flight rotation candidate key.
const KEY_NEXT: &str = "password_next";

/// Build a [`KopiaEnv`] for `password` from a creds lease + the group's config.
fn kopia_env(lease: &CredsLease, config: &ServerGroupBackupConfig, password: String) -> KopiaEnv {
	KopiaEnv {
		creds_uri: lease.uri().to_string(),
		creds_token: lease.token().to_string(),
		region: config.region.clone(),
		password,
	}
}

/// Apply an exact keyset to the group's repo-password Secret.
async fn put(worker: &Worker, name: &str, keys: &[(&str, &str)]) -> Result<()> {
	let map: BTreeMap<String, String> = keys
		.iter()
		.map(|(k, v)| (k.to_string(), v.to_string()))
		.collect();
	worker
		.secrets
		.put_keys(name, &map)
		.await
		.map_err(|e| anyhow!("put secret {name}: {e}"))
}

/// Rotate a group's repo passphrase. Reconciles any prior half-done rotation
/// first, then runs the dual-key dance. Caller holds the group's in-flight slot.
pub async fn rotate(worker: &Worker, config: &ServerGroupBackupConfig) -> Result<()> {
	reconcile(worker, config).await?;

	let keys = worker
		.secrets
		.read_keys(&config.repo_password_ref)
		.await
		.map_err(|e| anyhow!("read secret {}: {e}", config.repo_password_ref))?;
	let current = keys
		.get(KEY_CURRENT)
		.cloned()
		.ok_or_else(|| anyhow!("secret {} has no {KEY_CURRENT}", config.repo_password_ref))?;
	let new = generate_passphrase();

	rotate_to(worker, config, &current, &new).await
}

/// The dual-key rotation from `current` → `new` (also the import-rotate path,
/// where `current` is the operator-supplied passphrase).
pub async fn rotate_to(
	worker: &Worker,
	config: &ServerGroupBackupConfig,
	current: &str,
	new: &str,
) -> Result<()> {
	let secret_ref = &config.repo_password_ref;
	let lease = worker
		.creds
		.lease(&config.maintenance_role_arn, config.region.as_deref())
		.await?;
	let region = config.region.as_deref().unwrap_or_default();

	// 1. Persist the candidate BEFORE touching the repo (crash-safety).
	put(
		worker,
		secret_ref,
		&[(KEY_CURRENT, current), (KEY_NEXT, new)],
	)
	.await?;
	// 2. Rotate the repo (change_password verifies by reconnecting with `new`).
	kopia::change_password(
		&kopia_env(&lease, config, current.to_string()),
		&config.bucket,
		&config.prefix,
		region,
		new,
	)
	.await?;
	// 3. Promote: apply only `password=new`, which removes `password_next`.
	put(worker, secret_ref, &[(KEY_CURRENT, new)]).await?;
	Ok(())
}

/// Outcome of inspecting a (possibly) half-done rotation.
#[derive(Debug, PartialEq, Eq)]
enum Recovery {
	/// No rotation in flight (no distinct candidate) — nothing to do.
	Noop,
	/// Repo still on `password` → change-password never committed; drop candidate.
	Abandon,
	/// Repo on `password_next` → promote didn't persist; finish it.
	Promote,
	/// Neither passphrase opens the repo — corrupt/locked; alert.
	Broken,
}

/// Decide recovery from the Secret state + which passphrases the repo accepts.
/// Pure, so the branching is unit-tested without kopia.
fn reconcile_decision(
	current: Option<&str>,
	next: Option<&str>,
	current_connects: bool,
	next_connects: bool,
) -> Recovery {
	match (current, next) {
		// A distinct candidate means a rotation was in flight.
		(Some(c), Some(n)) if c != n => {
			if current_connects {
				Recovery::Abandon
			} else if next_connects {
				Recovery::Promote
			} else {
				Recovery::Broken
			}
		}
		_ => Recovery::Noop,
	}
}

/// Finish or abandon a rotation that crashed mid-flight (idempotent; safe to
/// call before every rotation and on a recovery sweep).
pub async fn reconcile(worker: &Worker, config: &ServerGroupBackupConfig) -> Result<()> {
	let secret_ref = &config.repo_password_ref;
	let keys = worker
		.secrets
		.read_keys(secret_ref)
		.await
		.map_err(|e| anyhow!("read secret {secret_ref}: {e}"))?;
	// Only probe when there's a distinct in-flight candidate.
	let (Some(current), Some(next)) = (keys.get(KEY_CURRENT).cloned(), keys.get(KEY_NEXT).cloned())
	else {
		return Ok(());
	};
	if current == next {
		return Ok(());
	}

	let lease = worker
		.creds
		.lease(&config.maintenance_role_arn, config.region.as_deref())
		.await?;
	let region = config.region.as_deref().unwrap_or_default();
	let env_current = kopia_env(&lease, config, current.clone());
	let current_ok = kopia::connect(&env_current, &config.bucket, &config.prefix, region)
		.await
		.is_ok();
	let next_ok = if current_ok {
		false
	} else {
		let env_next = kopia_env(&lease, config, next.clone());
		kopia::connect(&env_next, &config.bucket, &config.prefix, region)
			.await
			.is_ok()
	};

	match reconcile_decision(Some(&current), Some(&next), current_ok, next_ok) {
		Recovery::Noop => Ok(()),
		Recovery::Abandon => {
			warn!(group = %config.group_id, "rotation reconcile: abandoning uncommitted candidate");
			put(worker, secret_ref, &[(KEY_CURRENT, &current)]).await
		}
		Recovery::Promote => {
			warn!(group = %config.group_id, "rotation reconcile: promoting committed candidate");
			put(worker, secret_ref, &[(KEY_CURRENT, &next)]).await
		}
		Recovery::Broken => {
			bail!(
				"rotation reconcile for group {}: neither passphrase opens the repo \
				 (possible kopia #3049 corruption — manual intervention required)",
				config.group_id
			)
		}
	}
}

// ===========================================================================
// Background rotation scheduler.
// ===========================================================================

/// Loop tick interval.
const TICK: Duration = Duration::from_secs(60);
/// Default rotation period (forward-protection cadence). Overridable via
/// `CANOPY_BACKUP_ROTATION_DAYS`; the design targets pushing this toward daily.
const DEFAULT_ROTATION_DAYS: u64 = 7;

fn rotation_period() -> Duration {
	let days = std::env::var("CANOPY_BACKUP_ROTATION_DAYS")
		.ok()
		.and_then(|s| s.parse::<u64>().ok())
		.filter(|d| *d > 0)
		.unwrap_or(DEFAULT_ROTATION_DAYS);
	Duration::from_secs(days * 24 * 3600)
}

fn secs_into(now: Timestamp, window: Duration) -> u64 {
	let w = window.as_secs().max(1) as i64;
	now.as_second().rem_euclid(w) as u64
}

/// Spawn a rotation op for a group (claims the shared per-group slot so it never
/// races maintenance/inspection/init for the same group).
fn spawn_rotate(worker: &Worker, config: ServerGroupBackupConfig) {
	let Some(guard) = worker.try_claim(config.group_id) else {
		return;
	};
	let worker = worker.clone();
	task::spawn(async move {
		let _guard = guard;
		match rotate(&worker, &config).await {
			Ok(()) => info!(group = %config.group_id, "passphrase rotated"),
			Err(e) => error!(group = %config.group_id, "passphrase rotation failed: {e:#}"),
		}
	});
}

async fn tick(worker: &Worker) -> Result<(), String> {
	let mut db = worker.pool.get().await.map_err(|e| e.to_string())?;
	let all = ServerGroupBackupConfig::list(&mut db)
		.await
		.map_err(|e| e.to_string())?;
	let now = Timestamp::now();
	let period = rotation_period();
	let in_flight = worker.in_flight_snapshot();

	for c in &all {
		if c.status != BackupConfigStatus::Ready || in_flight.contains(&c.group_id) {
			continue;
		}
		// Deterministic per-group slot within the period (hash-jittered), so the
		// fleet's rotations spread out and each group rotates ~once per period.
		if slot_is_due(c.group_id, period, TICK, secs_into(now, period)) {
			spawn_rotate(worker, c.clone());
		}
	}
	Ok(())
}

/// Run the background rotation scheduler (one tick/minute).
pub fn spawn(worker: Worker) -> JoinHandle<()> {
	task::spawn(async move {
		loop {
			sleep(TICK).await;
			if let Err(e) = tick(&worker).await {
				error!("rotation tick failed: {e}");
			} else {
				debug!("rotation tick ok");
			}
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reconcile_decision_covers_all_states() {
		// No candidate → noop.
		assert_eq!(
			reconcile_decision(Some("a"), None, true, true),
			Recovery::Noop
		);
		// Candidate equals current (stale) → noop.
		assert_eq!(
			reconcile_decision(Some("a"), Some("a"), false, false),
			Recovery::Noop
		);
		// Distinct candidate, old still works → abandon.
		assert_eq!(
			reconcile_decision(Some("old"), Some("new"), true, false),
			Recovery::Abandon
		);
		// Distinct candidate, repo on new → promote.
		assert_eq!(
			reconcile_decision(Some("old"), Some("new"), false, true),
			Recovery::Promote
		);
		// Distinct candidate, neither opens → broken.
		assert_eq!(
			reconcile_decision(Some("old"), Some("new"), false, false),
			Recovery::Broken
		);
	}
}
