use std::time::{Duration, Instant};

use clap::Parser;
use commons_errors::Result;
use database::{Db, servers::Server, statuses::NewStatus};
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use serde_json::json;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error};

async fn record_own_status(pool: Db, start: Instant) -> Result<()> {
	let mut db = pool.get().await?;
	let server = Server::own(&mut db).await?;

	let status = NewStatus {
		server_id: server.id,
		device_id: None,
		version: Some(env!("CARGO_PKG_VERSION").parse().unwrap()),
		extra: json!({
			"uptime": start.elapsed().as_millis(),
			"hostname": hostname::get().unwrap(),
		}),
	}
	.save(&mut db)
	.await?;

	debug!(id=%status.id, "Recorded own status");

	Ok(())
}

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	let start = Instant::now();
	task::spawn(async move {
		loop {
			if let Err(err) = record_own_status(pool.clone(), start).await {
				error!("Failed to record own status: {err}");
				continue;
			}
			sleep(Duration::from_secs(60)).await;
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
