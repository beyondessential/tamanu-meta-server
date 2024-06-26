use futures::TryFutureExt;
use launch::Db;
use rocket_db_pools::Database;
use statuses::ping_servers_and_save;

#[macro_use]
extern crate rocket;

pub(crate) mod helper_types;
pub(crate) mod launch;
pub(crate) mod schema;
pub(crate) mod servers;
pub(crate) mod statuses;
pub(crate) mod versions;
pub(crate) mod views;

#[rocket::main]
async fn main() {
	let ship = launch::rocket()
		.ignite()
		.await
		.expect("Rocket failed to ignite");

	let pool = Db::fetch(&ship)
		.expect("Failed to fetch database pool")
		.clone();

	let rocket = ship.launch().map_err(|err| {
		err.pretty_print();
		// pretty_print() side-effects logs the error, so we can drop its result
	});

	let pinger = rocket::tokio::task::spawn(async move {
		loop {
			rocket::tokio::time::sleep(std::time::Duration::from_secs(60)).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};

			ping_servers_and_save(&mut db).await;
		}
	})
	.map_err(|err| {
		error!("pinger task failed: {:?}", err);
		// do the same thing as above (error here, then return ())
	});

	rocket::tokio::try_join!(rocket, pinger).ok();
}
