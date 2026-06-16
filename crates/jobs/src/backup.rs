//! k8s Job manifest + spawning for the backup schedulers.
//!
//! The pure scheduler logic (jitter, due-ness, billing labels, retention
//! floor, `JobKind`) lives in [`commons_servers::backup_jobs`] and is reused
//! here; this module adds the kube-dependent pieces: building the Job manifest
//! ([`jobspec`]) and the [`spawn::JobSpawner`] abstraction over the kube API
//! (with a fake for tests, since spawning real Jobs in CI isn't feasible).
//!
//! It also hosts the scheduler/monitor loops themselves — [`preflight`],
//! [`maintenance`], [`inspection`], and [`s3_metrics`] — each exposing a
//! `spawn()` that drives an independent task. The single `backups` bin spawns
//! all four.

pub mod jobspec;
pub mod spawn;

pub mod inspection;
pub mod maintenance;
pub mod preflight;
pub mod s3_metrics;

pub use commons_servers::backup_jobs::{BillingLabels, JobKind, RetentionPolicy};
