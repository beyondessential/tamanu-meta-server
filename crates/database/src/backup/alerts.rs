//! Group-level alerting entrypoint for the backup-credentials system.
//!
//! [`raise_group_event`] is the single place that opens (or recovers) a
//! group-scoped incident, bypassing the per-server `is_monitored` gate. It is
//! consumed by:
//! - this crate's staleness/reconcile scan ([`crate::backup::staleness`],
//!   [`crate::backup::reconcile`]),
//! - the **inspection Job** component for [`refs::CORRUPTION`],
//! - **PGRO ingest** (later) for [`refs::RESTORE_VERIFICATION`].
//!
//! Owning it here means exactly one code path knows how to raise a group-level
//! issue without re-inheriting the monitored gate.

pub use crate::backup::refs;
// Re-exported from `issues` (which owns the private incident plumbing) so all
// callers reach it via the backup module: `database::backup::alerts::raise_group_event`.
pub use crate::issues::raise_group_event;
