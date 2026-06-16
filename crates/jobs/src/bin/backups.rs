//! Backups scheduler pod. One deployment that runs all four backup
//! control-plane loops against the same database, each as an independent
//! spawned task:
//!
//! - upstream preflight ([`jobs::backup::preflight`]),
//! - kopia maintenance scheduler ([`jobs::backup::maintenance`]),
//! - read-only inspection scheduler ([`jobs::backup::inspection`]),
//! - S3 CloudWatch metrics task ([`jobs::backup::s3_metrics`]).
//!
//! They share the `canopy-jobs` ServiceAccount; the per-Job kopia roles
//! (maintenance / inspection / etc.) stay separate.

use clap::Parser;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use tracing::info;

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
	let preflight = jobs::backup::preflight::spawn();
	let maintenance = jobs::backup::maintenance::spawn();
	let inspection = jobs::backup::inspection::spawn();
	let s3_metrics = jobs::backup::s3_metrics::spawn();
	tokio::try_join!(preflight, maintenance, inspection, s3_metrics).into_diagnostic()?;
	Ok(())
}
