#[macro_use]
extern crate rocket;

pub use db::Db;
use rocket::figment::Figment;
use rocket_db_pools::Database;

pub(crate) mod app;
pub(crate) mod db;
pub(crate) mod error;
pub mod pingtask;
pub(crate) mod schema;
pub(crate) mod views;

pub async fn server(with_tasks: bool) {
	use rocket_db_pools::Database as _;

	let ship = app::rocket()
		.ignite()
		.await
		.expect("Rocket failed to ignite");

	let pool = db::Db::fetch(&ship)
		.expect("Failed to fetch database pool")
		.clone();

	let rocket = ship.launch();

	if with_tasks {
		let pinger = pingtask::spawn(pool);

		rocket::tokio::select! {
			r = rocket => {
				match r {
					Ok(_) => info!("Rocket shut down gracefully"),
					Err(e) => drop(e.pretty_print()),
				}
			}
			p = pinger => {
				match p {
					Ok(()) => info!("Ping task shut down gracefully"),
					Err(e) => error!("Ping task shut down with an error: {e:?}"),
				}
			}
		}
	} else {
		match rocket.await {
			Ok(_) => info!("Rocket shut down gracefully"),
			Err(e) => drop(e.pretty_print()),
		}
	}
}

fn db_config_figment() -> Figment {
	use rocket::figment::providers::Serialized;

	let ship = app::rocket();

	let workers: usize = ship
		.figment()
		.extract_inner(rocket::Config::WORKERS)
		.unwrap_or_else(|_| rocket::Config::default().workers);

	ship.figment()
		.focus("databases.postgres")
		.join(Serialized::default("max_connections", workers * 4))
		.join(Serialized::default("connect_timeout", 5))
}

pub async fn db_pool() -> Result<db::Db, rocket::Error> {
	use rocket_db_pools::Pool as _;

	let figment = db_config_figment();

	Ok(<db::Db as Database>::Pool::init(&figment)
		.await
		.expect("Failed to fetch database pool")
		.clone()
		.into())
}

pub fn db_config() -> Result<rocket_db_pools::Config, Box<rocket::figment::Error>> {
	db_config_figment().extract().map_err(Box::new)
}
