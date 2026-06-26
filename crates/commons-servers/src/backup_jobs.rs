//! Shared helpers for the backup-credentials scheduler binaries
//! (`backup_preflight` here, plus maintenance/inspection in the sibling jobs
//! component). Kept in `commons-servers` so all three schedulers agree on the
//! jitter scheme.

use std::time::Duration;

use commons_errors::Result;
use commons_types::{
	Uuid,
	backup::{BackupConfigStatus, BackupPurpose, BackupType},
	server::{rank::ServerRank, tags::TagMap},
};
use database::{
	BackupRequest, BackupRun, BackupTypeDefault, ServerBackupCapability, ServerGroupBackupConfig,
	ServerGroupBackupSchedule,
};
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
/// the org floor: the schedule override wins; else the type default; else (or on
/// parse error) the floor baseline. The floor is enforced on the result **unless**
/// the winning source opted out (`allow_below_floor`, a dangerous per-config
/// toggle for backups we're not authorised to keep). Each source is an
/// `(json, allow_below_floor)` pair. Pure so the precedence/floor logic is
/// unit-testable without a DB.
fn resolve_policy(
	override_: Option<(Value, bool)>,
	default: Option<(Value, bool)>,
) -> RetentionPolicy {
	match override_.or(default) {
		Some((json, allow_below_floor)) => match serde_json::from_value::<RetentionPolicy>(json) {
			Ok(policy) if allow_below_floor => policy,
			Ok(policy) => policy.enforce_floor(),
			Err(_) => RetentionPolicy::floor_baseline(),
		},
		None => RetentionPolicy::floor_baseline(),
	}
}

/// Resolve the effective retention policy for each backup type **declared** in
/// the group (not just enabled): schedule override → type default → org floor.
/// The floor is enforced unless the winning source set `allow_below_floor`.
/// Returns one `(type, policy)` pair per declared type — so a manual backup of a
/// non-scheduled (disabled) type is still retained under its own type policy
/// rather than only the repo's global baseline.
pub async fn effective_retention_for_group(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
) -> Result<Vec<(BackupType, RetentionPolicy)>> {
	let types = ServerBackupCapability::declared_types_for_group(db, group_id).await?;
	let mut out = Vec::with_capacity(types.len());
	for ty in types {
		let override_ = ServerGroupBackupSchedule::get(db, group_id, &ty)
			.await?
			.and_then(|s| s.retention.map(|r| (r, s.allow_below_floor)));
		let default = BackupTypeDefault::get(db, &ty)
			.await?
			.map(|d| (d.default_retention, d.allow_below_floor));
		out.push((ty, resolve_policy(override_, default)));
	}
	Ok(out)
}

/// Resolve the effective backup interval for one `(group, type)`: the schedule
/// `expected_interval` override, else the type's `default_interval`. `None` ⇒
/// manual-only (no scheduled cadence).
async fn effective_interval_for_type(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
	ty: &BackupType,
) -> Result<Option<Duration>> {
	let interval = match ServerGroupBackupSchedule::get(db, group_id, ty)
		.await?
		.and_then(|s| s.expected_interval)
	{
		Some(i) => Some(i),
		None => BackupTypeDefault::get(db, ty)
			.await?
			.and_then(|d| d.default_interval),
	};
	// PgDuration wraps a jiff SignedDuration; whole seconds → Duration.
	Ok(interval.map(|pg| Duration::from_secs(pg.0.as_secs().max(0) as u64)))
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
		if let Some(d) = effective_interval_for_type(db, group_id, &ty).await? {
			min = Some(min.map_or(d, |m| m.min(d)));
		}
	}
	Ok(min)
}

/// The backup types a server should back up *now*: every enabled `(server,
/// type)` whose effective interval has elapsed since its last successful backup
/// (schedule-due), unioned with operator one-off [`BackupRequest`]s
/// (`purpose = backup` only — restore is operator-directed, not delivered over
/// the heartbeat). Manual-only types (no effective interval) appear only when
/// explicitly requested. Sorted by type name for a stable wire order.
///
/// Emitted idempotently each tick: a due type keeps appearing until a
/// successful run reports (advancing the staleness anchor), and a one-off until
/// the run is reported (which clears the request). The device is responsible
/// for not starting a second run while one is already in flight.
pub async fn backups_due_now_for_server(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Uuid,
	now: Timestamp,
) -> Result<Vec<BackupType>> {
	// Nothing to back up to unless the group's repo is ready — a request or a
	// due schedule against a still-provisioning config would only bounce off the
	// device's `/backup-target` (dormant). Gate the whole signal on readiness.
	match ServerGroupBackupConfig::get(db, group_id).await? {
		Some(cfg) if cfg.status == BackupConfigStatus::Ready => {}
		_ => return Ok(Vec::new()),
	}

	let mut due: std::collections::HashSet<BackupType> = std::collections::HashSet::new();

	for req in BackupRequest::pending_for_server(db, server_id).await? {
		if req.purpose == BackupPurpose::Backup {
			due.insert(req.r#type);
		}
	}

	for cap in ServerBackupCapability::list_for_server(db, server_id).await? {
		if !cap.enabled {
			continue;
		}
		let Some(interval) = effective_interval_for_type(db, group_id, &cap.r#type).await? else {
			continue;
		};
		let last = BackupRun::latest_success_for_server(db, server_id, &cap.r#type)
			.await?
			.map(|r| r.reported_at);
		if is_due(interval, last, now) {
			due.insert(cap.r#type);
		}
	}

	let mut out: Vec<BackupType> = due.into_iter().collect();
	out.sort_by(|a, b| a.as_str().cmp(b.as_str()));
	Ok(out)
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
	/// Explicit `billing.*` tags are honored **verbatim**; computed fallbacks are
	/// lower-kebab-cased — `product = "tamanu"`, `deployment = lower_kebab(group
	/// name)`, `stage = mapped highest rank` (omitted when the group has no ranked
	/// members).
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
				.unwrap_or_else(|| lower_kebab(group_name)),
			stage: tags
				.0
				.get("billing.stage")
				.cloned()
				.or_else(|| highest_rank.map(|r| stage_for_rank(r).to_string())),
		}
	}

	/// Override the `product` label — e.g. `"backups"` for a backup bucket, which
	/// should attribute to the backups product regardless of the group's own
	/// `billing.product`. (Reusable for other canopy-owned resources later.)
	pub fn with_product(mut self, product: impl Into<String>) -> Self {
		self.product = product.into();
		self
	}

	/// Render as AWS `billing.*` resource tags. `billing.stage` is omitted when
	/// the group has no ranked members (no stage to attribute to).
	pub fn into_tags(self) -> Vec<(String, String)> {
		let mut tags = vec![
			("billing.product".to_string(), self.product),
			("billing.deployment".to_string(), self.deployment),
		];
		if let Some(stage) = self.stage {
			tags.push(("billing.stage".to_string(), stage));
		}
		tags
	}
}

// ===========================================================================
// Shared-account backups: config from env + bucket naming.
// ===========================================================================

fn env_nonempty(key: &str) -> Option<String> {
	std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Shared-account backup settings from the `CANOPY_SHARED_BACKUP_*` env vars.
/// `Some` only when the feature is configured. `region`/`device_role_arn`/
/// `maintenance_role_arn` are required (private-server writes the role ARNs into
/// each shared config; public-server assumes the device role). `provisioner_role_arn`
/// is only used by the backups pod at provision time and is empty when unset —
/// the provision path checks it then.
#[derive(Clone, Debug)]
pub struct SharedBackupConfig {
	pub region: String,
	pub device_role_arn: String,
	pub maintenance_role_arn: String,
	pub provisioner_role_arn: String,
}

impl SharedBackupConfig {
	pub fn from_env() -> Option<Self> {
		Some(Self {
			region: env_nonempty("CANOPY_SHARED_BACKUP_REGION")?,
			device_role_arn: env_nonempty("CANOPY_SHARED_BACKUP_DEVICE_ROLE_ARN")?,
			maintenance_role_arn: env_nonempty("CANOPY_SHARED_BACKUP_MAINTENANCE_ROLE_ARN")?,
			provisioner_role_arn: env_nonempty("CANOPY_SHARED_BACKUP_PROVISIONER_ROLE_ARN")
				.unwrap_or_default(),
		})
	}
}

/// The fixed prefix of a canopy-provisioned shared bucket name.
pub const SHARED_BUCKET_PREFIX: &str = "bes-canopy-backup-";
const S3_BUCKET_MAX: usize = 63;

/// Lower-kebab-case a free-form string: lowercased, every run of non-`[a-z0-9]`
/// collapsed to a single `-`, leading/trailing `-` trimmed. May return empty.
fn lower_kebab(s: &str) -> String {
	let mut out = String::new();
	let mut prev_dash = false;
	for c in s.chars() {
		let lc = c.to_ascii_lowercase();
		if lc.is_ascii_lowercase() || lc.is_ascii_digit() {
			out.push(lc);
			prev_dash = false;
		} else if !prev_dash {
			out.push('-');
			prev_dash = true;
		}
	}
	out.trim_matches('-').to_string()
}

/// Sanitize an arbitrary string into an S3-bucket-name-safe segment:
/// [`lower_kebab`], then truncated to `max_len` (re-trimming any trailing `-`
/// left by truncation). May return empty.
fn sanitize_bucket_segment(s: &str, max_len: usize) -> String {
	let mut seg: String = lower_kebab(s).chars().take(max_len).collect();
	while seg.ends_with('-') {
		seg.pop();
	}
	seg
}

/// Build a canopy-provisioned shared bucket name
/// `bes-canopy-backup-<group>-<random>`, total ≤ 63. Only the group segment is
/// truncated, to the budget left by the fixed prefix + `-<random>`. `random` is
/// the caller-supplied unique suffix (assumed already S3-safe, e.g. hex). When
/// the group name sanitizes to empty, the segment (and its `-`) is dropped.
pub fn shared_bucket_name(group_name: &str, random: &str) -> String {
	let budget = S3_BUCKET_MAX.saturating_sub(SHARED_BUCKET_PREFIX.len() + 1 + random.len());
	let seg = sanitize_bucket_segment(group_name, budget);
	if seg.is_empty() {
		format!("{SHARED_BUCKET_PREFIX}{random}")
	} else {
		format!("{SHARED_BUCKET_PREFIX}{seg}-{random}")
	}
}

/// Billing tags for a canopy backup bucket. Built from the group's
/// [`BillingLabels`] — so a group's explicit `billing.deployment` /
/// `billing.stage` overrides are honored (keeping the bucket's cost attribution
/// consistent with the deployment's other resources) — with `billing.product`
/// **forced to `backups`** (backup spend attributes to the backups product
/// regardless of the group's own product). Applied at provision time and
/// re-applied by the reconcile pass on drift.
pub fn backup_bucket_billing_tags(
	group_tags: &TagMap,
	group_name: &str,
	highest_rank: Option<ServerRank>,
) -> Vec<(String, String)> {
	BillingLabels::from_group(group_tags, group_name, highest_rank)
		.with_product("backups")
		.into_tags()
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
			Some((serde_json::json!({"keep_daily": 30}), false)),
			Some((serde_json::json!({"keep_daily": 10}), false)),
		);
		assert_eq!(p.keep_daily, 30, "override wins");

		// No override → default applies.
		let p = resolve_policy(None, Some((serde_json::json!({"keep_monthly": 12}), false)));
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
			Some((
				serde_json::json!({"keep_daily": 2, "keep_weekly": 1}),
				false,
			)),
			None,
		);
		assert_eq!(p.keep_daily, 7, "clamped up");
		assert_eq!(p.keep_weekly, 4, "clamped up");

		// Garbage JSON → floor baseline (parse error path).
		let p = resolve_policy(Some((serde_json::json!("not a policy"), false)), None);
		assert_eq!(p.keep_daily, 7);
	}

	#[test]
	fn resolve_policy_allow_below_floor_skips_floor() {
		// A dangerous override below the floor is preserved verbatim, not clamped.
		let p = resolve_policy(
			Some((serde_json::json!({"keep_daily": 2, "keep_weekly": 0}), true)),
			None,
		);
		assert_eq!(p.keep_daily, 2, "below-floor preserved");
		assert_eq!(p.keep_weekly, 0, "below-floor preserved");

		// A dangerous default is also exempt when it's the winning source.
		let p = resolve_policy(None, Some((serde_json::json!({"keep_daily": 1}), true)));
		assert_eq!(p.keep_daily, 1, "dangerous default exempt");

		// The override's flag governs — a dangerous default doesn't exempt a
		// non-dangerous override (the override is the winning source).
		let p = resolve_policy(
			Some((serde_json::json!({"keep_daily": 3}), false)),
			Some((serde_json::json!({"keep_daily": 1}), true)),
		);
		assert_eq!(p.keep_daily, 7, "non-dangerous override still floored");
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

		// A computed deployment (group name) is lower-kebab-cased.
		let b = BillingLabels::from_group(&empty, "Acme Prod", None);
		assert_eq!(b.deployment, "acme-prod");

		// An explicit billing.deployment tag is honored verbatim (not kebab'd).
		let mut dep = TagMap::default();
		dep.0
			.insert("billing.deployment".into(), "Acme Prod".into());
		let b = BillingLabels::from_group(&dep, "ignored", None);
		assert_eq!(b.deployment, "Acme Prod");

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

	#[test]
	fn shared_bucket_name_basic() {
		let n = shared_bucket_name("Acme Prod", "deadbeef");
		assert_eq!(n, "bes-canopy-backup-acme-prod-deadbeef");
		assert!(n.len() <= 63);
	}

	#[test]
	fn shared_bucket_name_sanitizes_and_collapses() {
		// Mixed case, spaces, underscores, dots, and runs of junk → single dashes.
		let n = shared_bucket_name("Foo__Bar.Baz  / qux", "abc123");
		assert_eq!(n, "bes-canopy-backup-foo-bar-baz-qux-abc123");
	}

	#[test]
	fn shared_bucket_name_truncates_to_63_without_trailing_dash() {
		let long = "x".repeat(100);
		let n = shared_bucket_name(&long, "abcd1234");
		assert!(n.len() <= 63, "len {} > 63", n.len());
		assert!(n.starts_with("bes-canopy-backup-"));
		assert!(n.ends_with("-abcd1234"));
		assert!(!n.contains("--"), "no doubled dash from truncation");
	}

	#[test]
	fn shared_bucket_name_truncation_does_not_leave_trailing_dash() {
		// A name that, sanitized, would land a '-' exactly at the truncation edge.
		let name = format!("{}{}", "a".repeat(20), " z"); // budget pushes the split near the space
		let n = shared_bucket_name(&name, "abcd1234");
		let seg = n
			.strip_prefix(SHARED_BUCKET_PREFIX)
			.unwrap()
			.strip_suffix("-abcd1234")
			.unwrap();
		assert!(!seg.ends_with('-') && !seg.starts_with('-'));
	}

	#[test]
	fn shared_bucket_name_empty_group_drops_segment() {
		let n = shared_bucket_name("!!!", "deadbeef");
		assert_eq!(n, "bes-canopy-backup-deadbeef");
	}

	#[test]
	fn billing_tags_default_product_deployment_stage() {
		let t = backup_bucket_billing_tags(
			&TagMap::default(),
			"Acme Prod",
			Some(ServerRank::Production),
		);
		assert!(t.contains(&("billing.product".to_string(), "backups".to_string())));
		// Computed deployment (group name) is lower-kebab-cased.
		assert!(t.contains(&("billing.deployment".to_string(), "acme-prod".to_string())));
		// Production maps to "prod", not the Display "production".
		assert!(t.contains(&("billing.stage".to_string(), "prod".to_string())));
	}

	#[test]
	fn billing_tags_omit_stage_when_unranked() {
		let t = backup_bucket_billing_tags(&TagMap::default(), "g", None);
		assert!(!t.iter().any(|(k, _)| k == "billing.stage"));
	}

	#[test]
	fn billing_tags_honor_group_overrides_but_force_product() {
		let mut tags = TagMap::default();
		tags.0
			.insert("billing.deployment".into(), "custom-dep".into());
		tags.0.insert("billing.stage".into(), "staging".into());
		// A group billing.product override is deliberately ignored for buckets.
		tags.0.insert("billing.product".into(), "tamanu".into());
		let t = backup_bucket_billing_tags(&tags, "ignored-name", Some(ServerRank::Dev));
		// product is forced to backups despite the group's billing.product=tamanu.
		assert!(t.contains(&("billing.product".to_string(), "backups".to_string())));
		// deployment + stage honor the group's explicit overrides.
		assert!(t.contains(&("billing.deployment".to_string(), "custom-dep".to_string())));
		assert!(t.contains(&("billing.stage".to_string(), "staging".to_string())));
	}
}
