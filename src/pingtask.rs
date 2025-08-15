use std::time::Duration;

use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::error;

use crate::{db::statuses::Status, state::AppState};

pub fn spawn() -> JoinHandle<()> {
	let pool = AppState::init_db();
	task::spawn(async move {
		loop {
			sleep(Duration::from_secs(60)).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};

			if let Err(err) = Status::ping_servers_and_save(&mut db).await {
				error!("Failed to ping servers: {err}");
				continue;
			}
		}
	})
}
