use std::time::Duration;

use rocket::tokio::{
	task::{self, JoinHandle},
	time::sleep,
};

use crate::db::{statuses::Status, Db};

pub fn spawn(pool: Db) -> JoinHandle<()> {
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
