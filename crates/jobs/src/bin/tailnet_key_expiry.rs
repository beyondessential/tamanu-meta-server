//! Periodic sweep that opens a critical issue when an attached
//! Tailscale node still has key-expiry enabled (so its key will
//! expire and the node will drop off the tailnet). Closes the issue
//! once the operator pins the key.

use std::time::Duration;

use clap::Parser;
use commons_servers::{
	tailnet_directory::{TailnetDirectory, TailnetDirectoryConfig},
	tailnet_sweeps,
};
use lloggs::{LoggingArgs, PreArgs};
use miette::{IntoDiagnostic, miette};
use tokio::time::sleep;
use tracing::{debug, error, info};

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

	let config = TailnetDirectoryConfig::from_env()
		.into_diagnostic()?
		.ok_or_else(|| miette!("TAILSCALE_* env vars not set; tailnet sweep cannot run"))?;
	let directory = TailnetDirectory::new(config).await.into_diagnostic()?;
	let pool = database::init();

	info!("tailnet key-expiry sweep running");
	loop {
		sleep(Duration::from_secs(60)).await;
		let Ok(mut db) = pool.get().await else {
			error!("failed to get database connection");
			continue;
		};

		match tailnet_sweeps::sweep_key_expiry(&mut db, &directory).await {
			Ok(0) => {}
			Ok(n) => debug!("filed {n} tailscale key-expiry events"),
			Err(err) => error!("tailnet key-expiry sweep failed: {err}"),
		}
	}
}
