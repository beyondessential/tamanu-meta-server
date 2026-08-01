//! Shared worker state for the in-process backup loops.
//!
//! [`Worker`] is built once in the `backups` bin and shared by the maintenance
//! and inspection loops. It holds the DB pool, a [`BackupSecrets`] store (read +
//! write the per-group repo-password Secret), the [`Cfg`], a concurrency
//! [`Semaphore`], and the in-flight group set — so the same group isn't worked
//! by two ops at once (one op per group at a time across maintenance +
//! inspection + init).

use std::{
	collections::{HashMap, HashSet},
	sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow};
use commons_servers::backup_secrets::BackupSecrets;
use commons_types::backoff::Backoff;
use database::Db;
use jiff::{SignedDuration, Timestamp};
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

/// Wait after the first failure of a group's op, doubling with each consecutive
/// failure. The ceiling keeps a long-broken group retrying twice a day (in case
/// someone fixed it out-of-band) without occupying a permit every tick.
const OP_BACKOFF: Backoff = Backoff::new(
	SignedDuration::from_mins(15),
	SignedDuration::from_hours(12),
);

/// Which scheduler an op belongs to. Backoff is tracked per (group, op) so
/// a broken inspection doesn't hold back maintenance for the same group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpKind {
	Maintenance,
	Inspection,
}

/// Per-(group, op) retry backoff for failing kopia ops.
///
/// Both schedulers decide "is this due?" from the last *successful* run, so a
/// group whose op fails every time — a wrong passphrase Secret, a deleted
/// bucket, a revoked role — is re-spawned every tick forever. Each retry holds
/// one of the worker's few concurrency permits for the op's (often slow)
/// duration, so a handful of permanently-broken groups can occupy every permit
/// and stall maintenance, inspection and init for the entire healthy fleet.
/// `needs_init` avoids this by latching on `last_init_error`; nothing recorded
/// an inspection failure at all, so this is tracked in-process.
///
/// In-process, so a restart clears it: at worst the first tick after a restart
/// spends one retry per broken group, which is the same cost as a legitimate
/// recovery check.
#[derive(Clone, Default)]
pub struct Backoffs {
	state: Arc<Mutex<HashMap<(Uuid, OpKind), (u32, Timestamp)>>>,
}

impl Backoffs {
	/// Whether this group's op is still inside its backoff window.
	pub fn blocked(&self, group_id: Uuid, op: OpKind, now: Timestamp) -> bool {
		self.state
			.lock()
			.unwrap()
			.get(&(group_id, op))
			.is_some_and(|(consecutive, last_failed)| {
				now < *last_failed + OP_BACKOFF.after(*consecutive)
			})
	}

	/// Record a failure, lengthening the next wait.
	pub fn failed(&self, group_id: Uuid, op: OpKind, now: Timestamp) {
		let mut state = self.state.lock().unwrap();
		let entry = state.entry((group_id, op)).or_insert((0, now));
		entry.0 = entry.0.saturating_add(1);
		entry.1 = now;
	}

	/// Record a success, clearing any accumulated backoff.
	pub fn succeeded(&self, group_id: Uuid, op: OpKind) {
		self.state.lock().unwrap().remove(&(group_id, op));
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
	/// Retry backoff for ops that keep failing, so they can't monopolise the
	/// concurrency permits.
	pub backoffs: Backoffs,
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
			backoffs: Backoffs::default(),
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

	#[test]
	fn backoff_doubles_then_caps() {
		assert_eq!(OP_BACKOFF.after(1), SignedDuration::from_mins(15));
		assert_eq!(OP_BACKOFF.after(2), SignedDuration::from_mins(30));
		assert_eq!(OP_BACKOFF.after(3), SignedDuration::from_hours(1));
		assert_eq!(OP_BACKOFF.after(6), SignedDuration::from_hours(8));
		// Capped, and no overflow panic however long a group stays broken.
		assert_eq!(OP_BACKOFF.after(7), OP_BACKOFF.cap());
		assert_eq!(OP_BACKOFF.after(1000), OP_BACKOFF.cap());
		assert_eq!(OP_BACKOFF.after(u32::MAX), OP_BACKOFF.cap());
	}

	/// A group whose op always fails must not be re-spawned every tick: each
	/// retry holds one of the few concurrency permits, so a handful of broken
	/// groups can starve the healthy fleet.
	#[test]
	fn a_failing_op_is_blocked_until_its_backoff_elapses() {
		let b = Backoffs::default();
		let g = Uuid::from_u128(1);
		let t0: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();

		assert!(
			!b.blocked(g, OpKind::Maintenance, t0),
			"nothing has failed yet",
		);

		b.failed(g, OpKind::Maintenance, t0);
		assert!(b.blocked(g, OpKind::Maintenance, t0 + SignedDuration::from_mins(1)));
		assert!(b.blocked(g, OpKind::Maintenance, t0 + SignedDuration::from_mins(14)));
		assert!(
			!b.blocked(g, OpKind::Maintenance, t0 + SignedDuration::from_mins(16)),
			"retried once the first backoff elapses",
		);

		// A second consecutive failure waits twice as long.
		let t1 = t0 + SignedDuration::from_mins(16);
		b.failed(g, OpKind::Maintenance, t1);
		assert!(b.blocked(g, OpKind::Maintenance, t1 + SignedDuration::from_mins(29)));
		assert!(!b.blocked(g, OpKind::Maintenance, t1 + SignedDuration::from_mins(31)));
	}

	#[test]
	fn a_success_clears_the_backoff() {
		let b = Backoffs::default();
		let g = Uuid::from_u128(1);
		let t0: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
		b.failed(g, OpKind::Maintenance, t0);
		b.failed(g, OpKind::Maintenance, t0);
		b.succeeded(g, OpKind::Maintenance);
		assert!(!b.blocked(g, OpKind::Maintenance, t0));
		// And the count resets, so the next failure waits the base again.
		b.failed(g, OpKind::Maintenance, t0);
		assert!(!b.blocked(g, OpKind::Maintenance, t0 + SignedDuration::from_mins(16)));
	}

	/// A broken inspection must not hold back maintenance for the same group.
	#[test]
	fn backoff_is_tracked_per_op() {
		let b = Backoffs::default();
		let g = Uuid::from_u128(1);
		let t0: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
		b.failed(g, OpKind::Inspection, t0);
		assert!(b.blocked(g, OpKind::Inspection, t0));
		assert!(!b.blocked(g, OpKind::Maintenance, t0));
	}

	#[test]
	fn backoff_is_tracked_per_group() {
		let b = Backoffs::default();
		let t0: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
		b.failed(Uuid::from_u128(1), OpKind::Maintenance, t0);
		assert!(!b.blocked(Uuid::from_u128(2), OpKind::Maintenance, t0));
	}
}
