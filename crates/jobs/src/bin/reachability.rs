//! Periodic canopy reachability sweep. Once a minute, look at every
//! monitored server's latest status row and file (or close) the
//! `(source=canopy, ref=reachability)` issue. Severity escalates as the
//! server stays unreported — Notice → Warning at the sub-incident tiers,
//! then Error (opens an incident) at "Down", then Critical at "Gone".
//!
//! Independent of pingtask: most servers push their own status, so the
//! sweep operates on the resulting `statuses` rows regardless of which path
//! produced them.

use std::time::Duration;

use clap::Parser;
use database::statuses::Status;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error};

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
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
