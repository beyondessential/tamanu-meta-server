//! Shared helpers for the backup-credentials scheduler binaries
//! (`backup_preflight` here, plus maintenance/inspection in the sibling jobs
//! component). Kept in `commons-servers` so all three schedulers agree on the
//! jitter scheme.

use std::time::Duration;

use commons_errors::Result;
use commons_types::{
	Uuid,
	backup::BackupType,
	server::{rank::ServerRank, tags::TagMap},
};
use database::{BackupTypeDefault, ServerBackupCapability, ServerGroupBackupSchedule};
use diesel_async::AsyncPgConnection;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The kind of per-group backup Job the schedulers spawn. The string form is
/// the `canopy-backup-kind` label and the `generateName` segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
	MaintQuick,
	MaintFull,
	Inspect,
	Init,
}

impl JobKind {
	pub fn as_str(self) -> &'static str {
		match self {
			JobKind::MaintQuick => "maint-quick",
			JobKind::MaintFull => "maint-full",
			JobKind::Inspect => "inspect",
			JobKind::Init => "init",
		}
	}
}

/// A kopia keep-policy, mirroring the `retention` JSONB
/// (`server_group_backup_config` / `backup_type_defaults`). The org floor
/// (`keep_daily 7 / weekly 4 / monthly 6`) is enforced in code via
/// [`enforce_floor`](Self::enforce_floor); `keep_latest` is never floored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
	#[serde(default = "keep_latest_default")]
	pub keep_latest: u32,
	#[serde(default)]
	pub keep_daily: u32,
	#[serde(default)]
	pub keep_weekly: u32,
	#[serde(default)]
	pub keep_monthly: u32,
	#[serde(default)]
	pub keep_annual: u32,
}

fn keep_latest_default() -> u32 {
	1
}

impl RetentionPolicy {
	pub const FLOOR_DAILY: u32 = 7;
	pub const FLOOR_WEEKLY: u32 = 4;
	pub const FLOOR_MONTHLY: u32 = 6;

	/// Raise any sub-floor keep value up to the org minimum (a per-group value
	/// may only raise, never lower). `keep_latest`/`keep_annual` untouched.
	pub fn enforce_floor(mut self) -> Self {
		self.keep_daily = self.keep_daily.max(Self::FLOOR_DAILY);
		self.keep_weekly = self.keep_weekly.max(Self::FLOOR_WEEKLY);
		self.keep_monthly = self.keep_monthly.max(Self::FLOOR_MONTHLY);
		self
	}

	/// The org-floor baseline: a zero policy with the floor applied (RetentionPolicy
	/// has no `Default`, so we build it explicitly here).
	fn floor_baseline() -> Self {
		RetentionPolicy {
			keep_latest: keep_latest_default(),
			keep_daily: 0,
			keep_weekly: 0,
			keep_monthly: 0,
			keep_annual: 0,
		}
		.enforce_floor()
	}
}

/// Merge a per-`(group, type)` retention override with the type's default and
/// the org floor: the schedule override JSON wins; else the type default JSON;
/// else (or on parse error) the floor baseline. The result always has the floor
/// enforced. Pure so the precedence/floor logic is unit-testable without a DB.
fn resolve_policy(override_json: Option<Value>, default_json: Option<Value>) -> RetentionPolicy {
	let json = override_json.or(default_json);
	match json.map(serde_json::from_value::<RetentionPolicy>) {
		Some(Ok(policy)) => policy.enforce_floor(),
		_ => RetentionPolicy::floor_baseline(),
	}
}

/// Resolve the effective retention policy for each backup type enabled in the
/// group: schedule override → type default → org floor, with the floor always
/// enforced. Returns one `(type, policy)` pair per enabled type.
pub async fn effective_retention_for_group(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
) -> Result<Vec<(BackupType, RetentionPolicy)>> {
	let types = ServerBackupCapability::enabled_types_for_group(db, group_id).await?;
	let mut out = Vec::with_capacity(types.len());
	for ty in types {
		let override_json = ServerGroupBackupSchedule::get(db, group_id, &ty)
			.await?
			.and_then(|s| s.retention);
		let default_json = BackupTypeDefault::get(db, &ty)
			.await?
			.map(|d| d.default_retention);
		out.push((ty, resolve_policy(override_json, default_json)));
	}
	Ok(out)
}

/// Resolve the effective backup interval for the group: per enabled type, the
/// schedule `expected_interval` else the type `default_interval`; the group's
/// effective cadence is the MINIMUM across types (the most-frequent type drives
/// it). `None` when no enabled type has any interval.
pub async fn effective_interval_for_group(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
) -> Result<Option<Duration>> {
	let types = ServerBackupCapability::enabled_types_for_group(db, group_id).await?;
	let mut min: Option<Duration> = None;
	for ty in types {
		let schedule_interval = ServerGroupBackupSchedule::get(db, group_id, &ty)
			.await?
			.and_then(|s| s.expected_interval);
		let interval = match schedule_interval {
			Some(i) => Some(i),
			None => BackupTypeDefault::get(db, &ty)
				.await?
				.and_then(|d| d.default_interval),
		};
		if let Some(pg) = interval {
			// PgDuration wraps a jiff SignedDuration; whole seconds → Duration.
			let secs = pg.0.as_secs().max(0) as u64;
			let d = Duration::from_secs(secs);
			min = Some(min.map_or(d, |m| m.min(d)));
		}
	}
	Ok(min)
}

/// The three `billing.*` pod labels spawned Jobs carry for AWS cost allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingLabels {
	pub product: String,
	pub deployment: String,
	/// `None` ⇒ omit the `billing.stage` label (group with no ranked members);
	/// a wrong `prod` would mis-attribute cost.
	pub stage: Option<String>,
}

/// Map a [`ServerRank`] to the CUR stage string ops already emits. **Gotcha:**
/// `Production` maps to `"prod"`, NOT the `Display` form `"production"`; the
/// others coincide but are mapped explicitly so a `Display` rename can't
/// silently break CUR tags.
pub fn stage_for_rank(rank: ServerRank) -> &'static str {
	match rank {
		ServerRank::Production => "prod",
		ServerRank::Clone => "clone",
		ServerRank::Demo => "demo",
		ServerRank::Test => "test",
		ServerRank::Dev => "dev",
	}
}

impl BillingLabels {
	/// Derive labels from a group's tags, name, and highest member rank.
	/// Explicit `billing.*` tags win; otherwise `product = "tamanu"`,
	/// `deployment = group name`, `stage = mapped highest rank` (omitted when
	/// the group has no ranked members).
	pub fn from_group(tags: &TagMap, group_name: &str, highest_rank: Option<ServerRank>) -> Self {
		BillingLabels {
			product: tags
				.0
				.get("billing.product")
				.cloned()
				.unwrap_or_else(|| "tamanu".to_string()),
			deployment: tags
				.0
				.get("billing.deployment")
				.cloned()
				.unwrap_or_else(|| group_name.to_string()),
			stage: tags
				.0
				.get("billing.stage")
				.cloned()
				.or_else(|| highest_rank.map(|r| stage_for_rank(r).to_string())),
		}
	}
}

/// Has a full cadence `window` elapsed since this kind of work last ran for a
/// group? `None` (never run) ⇒ due. Combine with [`slot_is_due`] in the loop so
/// the spawn lands on the group's stable jittered slot within the window.
pub fn is_due(window: Duration, last: Option<Timestamp>, now: Timestamp) -> bool {
	match last {
		None => true,
		Some(last) => {
			now.duration_since(last) >= SignedDuration::from_secs(window.as_secs() as i64)
		}
	}
}

/// Stable per-group jitter slot: `hash(group_id) mod window`.
///
/// Spreads per-group work (maintenance, inspection, preflight) evenly across
/// the cadence window so the fleet doesn't stampede at the top of the hour.
/// Derived deterministically from the group UUID's bytes, so it is stable
/// across restarts and identical in every scheduler — a given group always
/// lands in the same slot.
pub fn jitter_slot(group_id: Uuid, window: Duration) -> Duration {
	let window_secs = window.as_secs().max(1);
	let bytes = group_id.as_bytes();
	// Fold both halves so any byte difference changes the slot (UUIDs that
	// differ only in their low bytes must not collide).
	let hi = u64::from_be_bytes(bytes[..8].try_into().expect("uuid is 16 bytes"));
	let lo = u64::from_be_bytes(bytes[8..].try_into().expect("uuid is 16 bytes"));
	Duration::from_secs((hi ^ lo) % window_secs)
}

/// Whether `now` (as a count of seconds into the window) falls in this group's
/// jittered slot for a tick of length `tick`. Used by the minute-cadence
/// preflight loop to fire a group's hourly deep check on the right tick.
pub fn slot_is_due(
	group_id: Uuid,
	window: Duration,
	tick: Duration,
	secs_into_window: u64,
) -> bool {
	let slot = jitter_slot(group_id, window).as_secs();
	let tick_secs = tick.as_secs().max(1);
	// True when secs_into_window is within [slot, slot + tick).
	secs_into_window >= slot && secs_into_window < slot + tick_secs
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn jitter_is_stable_and_bounded() {
		let g = Uuid::from_u128(0x1234_5678_9abc_def0_1122_3344_5566_7788);
		let window = Duration::from_secs(3600);
		let a = jitter_slot(g, window);
		let b = jitter_slot(g, window);
		assert_eq!(a, b, "stable per group");
		assert!(a.as_secs() < 3600, "within the window");
	}

	#[test]
	fn different_groups_can_get_different_slots() {
		let window = Duration::from_secs(3600);
		let a = jitter_slot(Uuid::from_u128(1), window);
		let b = jitter_slot(Uuid::from_u128(2), window);
		assert_ne!(a, b);
	}

	#[test]
	fn slot_due_only_in_its_tick() {
		let g = Uuid::from_u128(7);
		let window = Duration::from_secs(3600);
		let tick = Duration::from_secs(60);
		let slot = jitter_slot(g, window).as_secs();
		assert!(slot_is_due(g, window, tick, slot));
		assert!(slot_is_due(g, window, tick, slot + 59));
		assert!(!slot_is_due(g, window, tick, slot + 60));
		if slot >= 1 {
			assert!(!slot_is_due(g, window, tick, slot - 1));
		}
	}

	#[test]
	fn retention_floor_raises_below_keeps_above() {
		let r = RetentionPolicy {
			keep_latest: 1,
			keep_daily: 3,
			keep_weekly: 4,
			keep_monthly: 99,
			keep_annual: 0,
		}
		.enforce_floor();
		assert_eq!(r.keep_daily, 7, "raised to floor");
		assert_eq!(r.keep_weekly, 4, "at floor, unchanged");
		assert_eq!(r.keep_monthly, 99, "above floor preserved");
		assert_eq!(r.keep_latest, 1, "keep_latest never floored");
	}

	#[test]
	fn resolve_policy_precedence_and_floor() {
		// Override wins over default.
		let p = resolve_policy(
			Some(serde_json::json!({"keep_daily": 30})),
			Some(serde_json::json!({"keep_daily": 10})),
		);
		assert_eq!(p.keep_daily, 30, "override wins");

		// No override → default applies.
		let p = resolve_policy(None, Some(serde_json::json!({"keep_monthly": 12})));
		assert_eq!(p.keep_monthly, 12, "default fallback");
		assert_eq!(p.keep_daily, 7, "floor still enforced on default");

		// Neither present → floor baseline.
		let p = resolve_policy(None, None);
		assert_eq!(p.keep_daily, 7);
		assert_eq!(p.keep_weekly, 4);
		assert_eq!(p.keep_monthly, 6);
		assert_eq!(p.keep_latest, 1);

		// Below-floor override → clamped up to the floor.
		let p = resolve_policy(
			Some(serde_json::json!({"keep_daily": 2, "keep_weekly": 1})),
			None,
		);
		assert_eq!(p.keep_daily, 7, "clamped up");
		assert_eq!(p.keep_weekly, 4, "clamped up");

		// Garbage JSON → floor baseline (parse error path).
		let p = resolve_policy(Some(serde_json::json!("not a policy")), None);
		assert_eq!(p.keep_daily, 7);
	}

	#[test]
	fn retention_json_roundtrip_defaults() {
		// keep_latest defaults to 1, others to 0, when absent.
		let r: RetentionPolicy = serde_json::from_str(r#"{"keep_daily":7}"#).unwrap();
		assert_eq!(r.keep_latest, 1);
		assert_eq!(r.keep_daily, 7);
		assert_eq!(r.keep_weekly, 0);
	}

	#[test]
	fn stage_mapping_production_is_prod() {
		assert_eq!(stage_for_rank(ServerRank::Production), "prod");
		assert_eq!(stage_for_rank(ServerRank::Clone), "clone");
		assert_eq!(stage_for_rank(ServerRank::Dev), "dev");
	}

	#[test]
	fn billing_labels_defaults_and_overrides() {
		// All-unranked group: no stage label, defaults for the rest.
		let empty = TagMap::default();
		let b = BillingLabels::from_group(&empty, "my-group", None);
		assert_eq!(b.product, "tamanu");
		assert_eq!(b.deployment, "my-group");
		assert_eq!(b.stage, None);

		// Highest rank maps in when present.
		let b = BillingLabels::from_group(&empty, "g", Some(ServerRank::Production));
		assert_eq!(b.stage.as_deref(), Some("prod"));

		// Explicit billing.* tags win.
		let mut tags = TagMap::default();
		tags.0.insert("billing.product".into(), "pgro".into());
		tags.0.insert("billing.stage".into(), "staging".into());
		let b = BillingLabels::from_group(&tags, "g", Some(ServerRank::Production));
		assert_eq!(b.product, "pgro");
		assert_eq!(b.stage.as_deref(), Some("staging"));
	}

	#[test]
	fn is_due_never_run_and_elapsed() {
		let window = Duration::from_secs(86400);
		let now: Timestamp = "2026-06-16T12:00:00Z".parse().unwrap();
		assert!(is_due(window, None, now), "never run is due");
		let recent: Timestamp = "2026-06-16T06:00:00Z".parse().unwrap();
		assert!(
			!is_due(window, Some(recent), now),
			"6h ago, 24h window: not due"
		);
		let old: Timestamp = "2026-06-15T06:00:00Z".parse().unwrap();
		assert!(is_due(window, Some(old), now), "30h ago, 24h window: due");
	}
}
