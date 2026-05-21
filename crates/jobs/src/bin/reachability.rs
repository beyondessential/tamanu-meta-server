//! Periodic canopy reachability sweep. Once a minute, look at every
//! monitored server's latest status row and file (or close) the
//! `(source=canopy, ref=reachability)` issue. Severity escalates as the
//! server stays unreported — Notice → Warning at the sub-incident tiers,
//! then Error (opens an incident) at "Down", then Critical at "Gone".
//!
//! Independent of pingtask: most servers push their own status, so the
//! sweep operates on the resulting `statuses` rows regardless of which path
//! produced them.
//!
//! The same loop also runs the tailnet key-expiry sweep when the
//! Tailscale directory is configured — both are minute-cadence reads
//! against the same DB, so there's no reason to stand up a second
//! deployment for it.

use std::time::Duration;

use clap::Parser;
use commons_servers::{
	tailnet_directory::{TailnetDirectory, TailnetDirectoryConfig},
	tailnet_sweeps,
};
use database::statuses::Status;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info};

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		// The directory is optional: in dev / single-tenant deploys the
		// TAILSCALE_* env vars aren't set, and the key-expiry sweep just
		// no-ops. Build it once at startup so we don't re-mint OAuth
		// tokens every cycle.
		let directory = match TailnetDirectoryConfig::from_env() {
			Ok(Some(config)) => match TailnetDirectory::new(config).await {
				Ok(d) => {
					info!("tailnet directory loaded; key-expiry sweep enabled");
					Some(d)
				}
				Err(err) => {
					error!("tailnet directory init failed; key-expiry sweep disabled: {err}");
					None
				}
			},
			Ok(None) => {
				info!("tailnet directory not configured; key-expiry sweep disabled");
				None
			}
			Err(err) => {
				error!("tailnet directory config invalid; key-expiry sweep disabled: {err}");
				None
			}
		};

		loop {
			sleep(Duration::from_secs(60)).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};

			match Status::sweep_reachability(&mut db).await {
				Ok(0) => {}
				Ok(n) => debug!("filed {n} reachability events"),
				Err(err) => error!("reachability sweep failed: {err}"),
			}

			if let Some(directory) = &directory {
				match tailnet_sweeps::sweep_key_expiry(&mut db, directory).await {
					Ok(0) => {}
					Ok(n) => debug!("filed {n} tailscale key-expiry events"),
					Err(err) => error!("tailnet key-expiry sweep failed: {err}"),
				}
			}
		}
	})
}

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

	spawn().await.into_diagnostic()?;
	Ok(())
}
