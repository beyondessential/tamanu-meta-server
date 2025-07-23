#[macro_use]
extern crate rocket;

pub use db::Db;
use rocket::figment::Figment;
use rocket_db_pools::Database;

pub(crate) mod db;
pub(crate) mod error;
pub mod pingtask;
pub(crate) mod schema;
pub(crate) mod servers;
pub(crate) mod views;

pub async fn public_server() {
	let ship = servers::public::rocket()
		.ignite()
		.await
		.expect("Rocket failed to ignite");

	match ship.launch().await {
		Ok(_) => info!("Rocket shut down gracefully"),
		Err(e) => drop(e.pretty_print()),
	}
}

pub async fn private_server(prefix: String) {
	let ship = servers::private::rocket(prefix)
		.ignite()
		.await
		.expect("Rocket failed to ignite");

	match ship.launch().await {
		Ok(_) => info!("Rocket shut down gracefully"),
		Err(e) => drop(e.pretty_print()),
	}
}

fn db_config_figment() -> Figment {
	use rocket::figment::providers::Serialized;

	let ship = servers::public::rocket();

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
