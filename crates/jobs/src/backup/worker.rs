//! Shared worker state for the in-process backup loops.
//!
//! [`Worker`] is built once in the `backups` bin and shared by the maintenance
//! and inspection loops. It holds the DB pool, a [`BackupSecrets`] store (read +
//! write the per-group repo-password Secret), the [`Cfg`], a concurrency
//! [`Semaphore`], and the in-flight group set — so the same group isn't worked
//! by two ops at once (one op per group at a time across maintenance +
//! inspection + init).

use std::{
	collections::HashSet,
	sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow};
use commons_servers::backup_secrets::BackupSecrets;
use database::Db;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use super::creds_server::CredsServer;

/// Default max concurrent kopia ops across both loops.
const DEFAULT_MAX_CONCURRENCY: usize = 4;

/// Scheduler config read from the environment, so one binary works across
/// stacks.
pub struct Cfg {
	/// Key within each repo-password Secret. (The namespace is handled by
	/// [`BackupSecrets`] via `POD_NAMESPACE`.)
	pub password_key: String,
}

impl Cfg {
	pub fn from_env() -> Self {
		Cfg {
			password_key: env_or("CANOPY_BACKUP_PASSWORD_KEY", "password"),
		}
	}
}

fn env_or(key: &str, default: &str) -> String {
	std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Concurrency + per-group in-flight gating, kept separate from the kube/DB
/// handles so it's unit-testable without a cluster. Enforces one op per group
/// at a time plus a global concurrency cap.
#[derive(Clone)]
pub struct Slots {
	semaphore: Arc<Semaphore>,
	in_flight: Arc<Mutex<HashSet<Uuid>>>,
}

impl Slots {
	pub fn new(max: usize) -> Self {
		Slots {
			semaphore: Arc::new(Semaphore::new(max.max(1))),
			in_flight: Arc::new(Mutex::new(HashSet::new())),
		}
	}

	/// Snapshot of the currently in-flight group ids (cheap pre-check; the real
	/// gate is [`try_claim`](Self::try_claim)).
	pub fn in_flight_snapshot(&self) -> HashSet<Uuid> {
		self.in_flight.lock().unwrap().clone()
	}

	/// Try to claim a group + a concurrency permit for one op. Returns an
	/// [`InFlightGuard`] (which releases both on drop) if the group is free and a
	/// permit is available; `None` otherwise (skip this group this tick).
	pub fn try_claim(&self, group_id: Uuid) -> Option<InFlightGuard> {
		// Take the permit first so we don't mark a group in-flight when we're at
		// the concurrency cap.
		let permit = Arc::clone(&self.semaphore).try_acquire_owned().ok()?;
		{
			let mut set = self.in_flight.lock().unwrap();
			if !set.insert(group_id) {
				return None; // already in-flight
			}
		}
		Some(InFlightGuard {
			in_flight: Arc::clone(&self.in_flight),
			group_id,
			_permit: permit,
		})
	}
}

/// Shared, cheaply-cloneable worker state for the maintenance + inspection
/// loops.
#[derive(Clone)]
pub struct Worker {
	pub pool: Db,
	/// Read/write the per-group repo-password Secret.
	pub secrets: BackupSecrets,
	pub cfg: Arc<Cfg>,
	pub slots: Slots,
	/// Loopback endpoint that mints per-op maintenance-role creds for kopia.
	pub creds: CredsServer,
}

impl Worker {
	pub fn new(pool: Db, secrets: BackupSecrets, cfg: Cfg, creds: CredsServer) -> Self {
		let max = std::env::var("CANOPY_BACKUP_MAX_CONCURRENCY")
			.ok()
			.and_then(|s| s.parse::<usize>().ok())
			.filter(|n| *n > 0)
			.unwrap_or(DEFAULT_MAX_CONCURRENCY);
		Worker {
			pool,
			secrets,
			cfg: Arc::new(cfg),
			slots: Slots::new(max),
			creds,
		}
	}

	/// Snapshot of the currently in-flight group ids (delegates to [`Slots`]).
	pub fn in_flight_snapshot(&self) -> HashSet<Uuid> {
		self.slots.in_flight_snapshot()
	}

	/// Try to claim a group + a concurrency permit (delegates to [`Slots`]).
	pub fn try_claim(&self, group_id: Uuid) -> Option<InFlightGuard> {
		self.slots.try_claim(group_id)
	}

	/// Read the repo passphrase from a group's k8s Secret.
	pub async fn read_repo_password(&self, secret_name: &str) -> Result<String> {
		self.secrets
			.read_password(secret_name, &self.cfg.password_key)
			.await
			.map_err(|e| anyhow!("reading secret {secret_name}: {e}"))
	}

	/// Create-or-replace a group's repo passphrase Secret (the rotation loop
	/// publishes a rotated passphrase here after a successful `change-password`).
	pub async fn write_repo_password(&self, secret_name: &str, value: &str) -> Result<()> {
		self.secrets
			.put_password(secret_name, &self.cfg.password_key, value)
			.await
			.map_err(|e| anyhow!("writing secret {secret_name}: {e}"))
	}
}

/// Releases a group's in-flight claim (and the concurrency permit) on drop.
pub struct InFlightGuard {
	in_flight: Arc<Mutex<HashSet<Uuid>>>,
	group_id: Uuid,
	_permit: OwnedSemaphorePermit,
}

impl Drop for InFlightGuard {
	fn drop(&mut self) {
		self.in_flight.lock().unwrap().remove(&self.group_id);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn in_flight_excludes_same_group() {
		let s = Slots::new(4);
		let g = Uuid::from_u128(1);
		let guard = s.try_claim(g).expect("first claim ok");
		assert!(s.try_claim(g).is_none(), "same group is excluded");
		let g2 = Uuid::from_u128(2);
		assert!(s.try_claim(g2).is_some(), "different group is allowed");
		drop(guard);
		assert!(s.try_claim(g).is_some(), "claim available after drop");
	}

	#[test]
	fn semaphore_caps_concurrency() {
		let s = Slots::new(2);
		let _g1 = s.try_claim(Uuid::from_u128(1)).expect("1 ok");
		let _g2 = s.try_claim(Uuid::from_u128(2)).expect("2 ok");
		assert!(
			s.try_claim(Uuid::from_u128(3)).is_none(),
			"third claim blocked by the concurrency cap"
		);
	}
}
