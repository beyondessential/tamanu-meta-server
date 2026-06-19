//! In-process backup control plane for the long-lived `backups` Deployment.
//!
//! The pure scheduler logic (jitter, due-ness, billing labels, retention floor,
//! `JobKind`) lives in [`commons_servers::backup_jobs`] and is reused here. The
//! kopia ops themselves run **in-process** as subprocesses ([`kopia`]) rather
//! than as Kubernetes Jobs; their typed outcomes are written to the DB by
//! [`complete`].
//!
//! It also hosts the scheduler/monitor loops themselves — [`preflight`],
//! [`maintenance`], [`inspection`], and [`s3_metrics`]. The maintenance and
//! inspection loops share a [`worker::Worker`] (DB pool, kube client for Secret
//! reads, concurrency semaphore, and the in-flight group set); preflight and
//! s3-metrics keep building their own pool/AWS clients. The single `backups` bin
//! spawns all four.

pub mod complete;
pub mod creds_server;
pub mod kopia;
pub mod worker;

pub mod inspection;
pub mod maintenance;
pub mod preflight;
pub mod s3_metrics;
