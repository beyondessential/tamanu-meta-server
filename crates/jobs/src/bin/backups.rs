//! Backups scheduler pod. One long-lived Deployment that runs all four backup
//! control-plane loops against the same database, each as an independent spawned
//! task:
//!
//! - upstream preflight ([`jobs::backup::preflight`]),
//! - kopia maintenance scheduler ([`jobs::backup::maintenance`]),
//! - read-only inspection scheduler ([`jobs::backup::inspection`]),
//! - S3 CloudWatch metrics task ([`jobs::backup::s3_metrics`]).
//!
//! The maintenance + inspection loops run kopia **in-process** (subprocesses)
//! rather than spawning Kubernetes Jobs; they share a [`jobs::backup::worker::Worker`]
//! (DB pool, kube client for Secret reads, concurrency semaphore, in-flight
//! group set, and the container-creds endpoint), built once here. Preflight and
//! s3-metrics keep building their own pool + AWS clients.
//!
//! Each kopia subprocess gets its group's maintenance-role creds by polling the
//! loopback container-credentials endpoint ([`jobs::backup::creds_server`]),
//! which mints + refreshes them from the pod's shared `canopy-jobs` IRSA
//! identity.

use clap::Parser;
use lloggs::{LoggingArgs, PreArgs};
use miette::{IntoDiagnostic, miette};
use tracing::info;

use jobs::backup::creds_server::CredsServer;
use jobs::backup::worker::{Cfg, Worker};

#[derive(Debug, Parser)]
struct Args {
	#[command(flatten)]
	logging: LoggingArgs,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
	let mut _guard = PreArgs::parse().setup()?;
	let args = Args::parse();
	if _guard.is_none() {
		_guard = Some(args.logging.setup(|v| match v {
			0 => "info",
			1 => "debug",
			_ => "trace",
		})?);
	}
	info!("backups scheduler starting");

	// Shared worker state for the in-process kopia loops, built once.
	let pool = database::init();
	let kube = kube::Client::try_default()
		.await
		.map_err(|e| miette!("kube client init failed: {e}"))?;
	let creds = CredsServer::start()
		.await
		.map_err(|e| miette!("container-creds endpoint init failed: {e}"))?;
	let worker = Worker::new(pool, kube, Cfg::from_env(), creds);

	let preflight = jobs::backup::preflight::spawn();
	let maintenance = jobs::backup::maintenance::spawn(worker.clone());
	let inspection = jobs::backup::inspection::spawn(worker);
	let s3_metrics = jobs::backup::s3_metrics::spawn();
	tokio::try_join!(preflight, maintenance, inspection, s3_metrics).into_diagnostic()?;
	Ok(())
}
