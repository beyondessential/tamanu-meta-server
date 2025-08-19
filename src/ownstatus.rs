use std::time::{Duration, Instant};

use serde_json::json;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error};

use crate::{
	db::{servers::Server, statuses::NewStatus},
	error::Result,
	state::{AppState, Db},
};

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
		..Default::default()
	}
	.save(&mut db)
	.await?;

	debug!(id=%status.id, "Recorded own status");

	Ok(())
}

pub fn spawn() -> JoinHandle<()> {
	let pool = AppState::init_db();
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
