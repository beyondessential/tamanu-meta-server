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

/// A run reported success but no matching repo snapshot landed (the device
/// lied or the upload didn't persist). Server-scoped, `Error`. Detecting it
/// takes the group's repo inventory, but the finding is about the one server
/// whose report didn't hold up, so it is filed against that server.
pub const RECONCILE_MISSING: &str = "backup-reconcile-missing";

// --- group-level (page regardless of any member's is_monitored) ---

/// A group whose last successful maintenance run is older than the
/// maintenance-cadence threshold. Group-scoped, `Error`.
pub const MAINTENANCE_STALE: &str = "backup-maintenance-stale";

/// A group whose most recently *finished* maintenance run failed. Distinct
/// from [`MAINTENANCE_STALE`] (which fires on absence of success): this fires
/// when maintenance is running but erroring. Group-scoped, `Error`. Clears
/// when a newer run finishes successfully.
pub const MAINTENANCE_ERROR: &str = "backup-maintenance-error";

/// Repo corruption / poisoning detected by the inspection Job. Group-scoped,
/// registers as an escalating failure. Raised by the inspection-Job component
/// via [`crate::issues::file_check`]; this constant is the contract.
pub const CORRUPTION: &str = "backup-corruption";

/// A rotation left the repo openable by neither the committed passphrase nor
/// the in-flight candidate. Backups and restores are both dead for the group
/// until someone intervenes. Group-scoped, escalating — Canopy cannot recover
/// from this on its own, so it must not wait behind incident grace.
pub const ROTATION_BROKEN: &str = "backup-rotation-broken";

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

/// PGRO reported a failed/stale restorability check for one replica.
/// Server-scoped, `Error`; the ref carries the `(type, intent)` dimension so
/// each replica of a server recovers independently.
pub const RESTORE_VERIFICATION: &str = "restore-verification";
pub const MIGRATION_TEST: &str = "migration-test";

/// The masking manifest for a redacting replica did not fully apply.
/// Server-scoped, `Warning`, does not escalate.
pub const REDACTION: &str = "redaction";

// --- shipped documentation (seeded into the catalog on first filing) ---

pub const STALENESS_DOC: &str = "## Description

The server has backed this type up successfully before, but not recently: the latest success is older than twice the expected interval for the type.

## Results

- **fail** — no successful run within 2\u{d7} the expected interval; recovers on the next successful run.

## Solve

Check the device's bestool logs for failed or stuck runs, confirm the backup schedule is enabled for the group, and run the backup manually (`bestool canopy backup`) to see the error first-hand. Credential or bucket problems usually show as separate preflight checks.";

pub const NEVER_DOC: &str = "## Description

The server is expected to back this type up but has never reported a single successful run, and it has been enrolled long enough that one should have happened.

## Results

- **fail** — no success ever, past the enrolment/schedule grace; recovers on the first successful run.

## Solve

Confirm bestool is installed and enrolled on the server, the backup type is enabled for its group, and run `bestool canopy backup` manually to surface the error.";

pub const RECONCILE_REPORT_GAP_DOC: &str = "## Description

A fresh snapshot for this server exists in the repository, but no recent run report reached canopy — the backup works, the reporting path doesn't.

## Results

- **warn** — snapshot fresh, report missing; recovers when a run report arrives.

## Solve

Check the device's connectivity to canopy and its bestool logs for failed report submissions. The data is safe; only visibility is degraded.";

pub const RECONCILE_SIZE_MISMATCH_DOC: &str = "## Description

The device reported a snapshot size that disagrees with the size the same snapshot occupies in the repository.

## Results

- **warn** — sizes disagree (compared only when both are known and non-zero).

## Solve

Usually a reporting bug or a partially-uploaded snapshot. Compare the run report against `kopia snapshot list` for the source, and re-run the backup if the repo-side size looks truncated.";

pub const MAINTENANCE_STALE_DOC: &str = "## Description

The group's repository maintenance (compaction, blob GC) hasn't succeeded within its cadence.

## Results

- **fail** — last successful maintenance run is older than the cadence threshold; recovers on the next success.

## Solve

Check the maintenance job's logs for the group. A repository that misses maintenance grows unbounded but stays fully restorable.";

pub const MAINTENANCE_ERROR_DOC: &str = "## Description

The group's most recently finished repository maintenance run failed (distinct from maintenance being absent).

## Results

- **fail** — latest finished run errored; clears when a newer run finishes successfully.

## Solve

Read the run's error in the group's backups panel. Common causes: credential expiry mid-run and repository lock contention.";

pub const RECONCILE_MISSING_DOC: &str = "## Description

A run reported success but no matching snapshot landed in the repository — the device's report and the repo disagree.

## Results

- **fail** — reported snapshot absent from the repo.

## Solve

Treat the backup as not having happened. Check the device's kopia logs for upload failures after the snapshot was cut, and re-run the backup.";

pub const CORRUPTION_DOC: &str = "## Description

The repository inspection job detected corruption or poisoning in the group's backup repository.

## Results

- **fail** — inspection found corrupt or unreadable data. Escalates: this is a threat to restorability itself.

## Solve

Do not run maintenance (it may GC evidence). Inspect the repository with kopia directly, identify the damaged blobs and affected snapshots, and restore repository health from object-lock history if needed.";

pub const ROTATION_BROKEN_DOC: &str = "## Description

A passphrase rotation was interrupted and left the repository openable by neither the committed passphrase nor the in-flight candidate — most likely the kopia format-blob corruption of kopia#3049, where a crash between the two format writes loses both.

Every device backup for the group fails against an unopenable repository, and no restore is possible either. Canopy cannot recover from this on its own: it holds the only copies of both passphrases and neither works.

## Results

- **fail** — neither passphrase opens the repo. Escalates: restorability is already gone, so this must not wait out incident grace.

## Solve

Recover the repository's format blob from object-lock history (the bucket keeps prior versions), then re-run the rotation. Do not run maintenance in the meantime — it may GC evidence. Until the repo opens again, treat the group as having no working backup.";

pub const PREFLIGHT_IDENTITY_DOC: &str = "## Description

Canopy's own AWS identity (`sts:GetCallerIdentity` under the shared IRSA role) failed — no backup credential can be minted for anyone.

## Results

- **fail** — the identity call errors. Escalates: every backup and restore across the fleet is blocked.

## Solve

Check the canopy deployment's IRSA annotation and the AWS-side trust policy; recent cluster or account changes are the usual cause.";

pub const PREFLIGHT_ASSUME_DOC: &str = "## Description

Cross-account `AssumeRole` (or the read-only no-op S3 probe) failed for this group's backup or restore leg.

## Results

- **fail** — the group's role can't be assumed or can't reach its bucket.

## Solve

Check the group's target role ARN and its trust policy against canopy's identity, and the bucket policy on the target account.";

pub const PREFLIGHT_OBJECT_LOCK_DOC: &str = "## Description

The group's backup bucket has missing or weakened Object-Lock protection (mode absent, or retention under 30 days).

## Results

- **fail** — protection below the floor. Escalates: backups are no longer ransomware-resistant.

## Solve

Restore the bucket's Object-Lock configuration to GOVERNANCE mode with at least 30 days retention. Investigate who changed it — this setting should never weaken.";

pub const RESTORE_VERIFICATION_DOC: &str = "## Description

The managed restore replica for this (server, type, intent) reported a failed or stale restorability check.

## Results

- **fail** — the replica couldn't restore or verify the latest snapshot.

## Solve

Check the restore consumer's report detail: restore errors point at the snapshot or credentials, staleness at the consumer itself.";

pub const MIGRATION_TEST_DOC: &str = "## Description

A candidate version's schema migrations were applied to a restore replica of this server's data, and one of them failed. The server itself is unaffected: it is still running the version it was, and the finding is about a version it has not taken.

## Results

- **warn**: the migrations did not complete against this deployment's data. The version carries a known issue and is held back from rollout.

## Solve

Read the failing migration named in the report detail. The fix belongs to the migration or to the deployment's data, and the version stays unready until someone resolves the known issue against it.";

pub const REDACTION_DOC: &str = "## Description

A replica of this server's data was declared to be served de-identified, and its masking manifest did not fully apply. The server itself is unaffected — this is about the copy, not the deployment.

## Results

- **warn (partial)** — the replica is live and most of its data is masked, but some columns could not be and are in the clear. The report detail carries how many; only the consumer's own logs name which.
- **warn (failed)** — no masking took effect, so the replica was not switched over. It is still serving the data it was already serving, which grows staler until the redaction succeeds.

## Solve

For a partial redaction, treat the replica as carrying real data until the skipped columns are identified from the consumer's logs — a column is usually skipped because its masking doesn't fit its type, which is a fix to the manifest.

For a failed one, the usual cause is the manifest being unreachable or not published for the version restored. Check that the version has a published manifest; a replica in this state stays on stale data indefinitely rather than serving anything unmasked.";
