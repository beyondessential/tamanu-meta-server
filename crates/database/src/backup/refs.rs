//! Stable `(source, ref)` alert keys for the backup-credentials system.
//!
//! `source` is always [`CANOPY_SOURCE`] (`"canopy"`) — the same constant the
//! reachability sweep uses. Operators silence/snooze by these keys via the
//! `silenced_refs` mechanism, and the UI / Slack reference them, so they are a
//! contract: do not rename without a coordinated migration of any stored
//! silences.

/// The event source for every backup alert. Re-exported from
/// [`crate::statuses::CANOPY_SOURCE`] so both the reachability sweep and the
/// backup jobs agree on one literal.
pub use crate::statuses::CANOPY_SOURCE;

// --- per-server (obey the is_monitored gate via NewEvent::save) ---

/// A prior successful backup exists but none recent (success older than 2×
/// the expected interval). Server-scoped, `Error`.
pub const STALENESS: &str = "backup-staleness";

/// No successful backup ever, and the server has been expected long enough
/// (past the `max(min_first_seen, schedule_created)` anchor + grace).
/// Server-scoped, `Error`.
pub const NEVER: &str = "backup-never";

/// A fresh repo snapshot exists for the server's source but no recent run was
/// reported — the *reporting* path is broken, not the backup. Server-scoped,
/// `Warning` (non-paging on its own).
pub const RECONCILE_REPORT_GAP: &str = "backup-reconcile-report-gap";

/// A device reported a snapshot size that disagrees with the size the same
/// snapshot occupies in the repo (compared only when both are known and
/// non-zero). Server-scoped, `Warning` (non-paging on its own).
pub const RECONCILE_SIZE_MISMATCH: &str = "backup-reconcile-size-mismatch";

// --- group-level (page regardless of any member's is_monitored) ---

/// A group whose last successful maintenance run is older than the
/// maintenance-cadence threshold. Group-scoped, `Error`.
pub const MAINTENANCE_STALE: &str = "backup-maintenance-stale";

/// A group whose most recently *finished* maintenance run failed. Distinct
/// from [`MAINTENANCE_STALE`] (which fires on absence of success): this fires
/// when maintenance is running but erroring. Group-scoped, `Error`. Clears
/// when a newer run finishes successfully.
pub const MAINTENANCE_ERROR: &str = "backup-maintenance-error";

/// A run reported success but no matching repo snapshot landed (the device
/// lied or the upload didn't persist). Group-scoped, `Error`.
pub const RECONCILE_MISSING: &str = "backup-reconcile-missing";

/// Repo corruption / poisoning detected by the inspection Job. Group-scoped,
/// registers as an escalating failure. Raised by the inspection-Job component
/// via [`crate::issues::file_canopy_check`]; this constant is the contract.
pub const CORRUPTION: &str = "backup-corruption";

/// Canopy's own `sts:GetCallerIdentity` failed — the shared IRSA identity is
/// broken. `Critical`. Filed once against the nil/meta server (one fact about
/// canopy, not one per group); recovery also clears any group-scoped issues
/// left from when this alert fanned out per group.
pub const PREFLIGHT_IDENTITY: &str = "preflight-identity";

/// Cross-account `AssumeRole` or the read-only no-op S3 call failed for a
/// group (either the backup or restore leg). Group-scoped, `Error`.
pub const PREFLIGHT_ASSUME: &str = "preflight-assume";

/// The bucket's Object-Lock configuration is missing or weakened (mode
/// absent, or retention < 30 days). Group-scoped, `Critical`.
pub const PREFLIGHT_OBJECT_LOCK: &str = "preflight-object-lock";

/// Restore-verification (later/additive): PGRO reported a failed/stale
/// restorability check. Group-scoped, `Error`. Routed through the same
/// group-level helper.
pub const RESTORE_VERIFICATION: &str = "restore-verification";
