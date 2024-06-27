use futures::TryFutureExt;
use rocket_db_pools::Database as _;

#[macro_use]
extern crate rocket;

pub(crate) mod app;
pub(crate) mod db;
pub(crate) mod pingtask;
pub(crate) mod schema;
pub(crate) mod views;

#[rocket::main]
async fn main() {
	let ship = app::rocket()
		.ignite()
		.await
		.expect("Rocket failed to ignite");

	let pool = db::Db::fetch(&ship)
		.expect("Failed to fetch database pool")
		.clone();

	let rocket = ship.launch().map_err(|err| {
		err.pretty_print();
		// pretty_print() side-effects logs the error, so we can drop its result
	});

	let pinger = pingtask::spawn(pool).map_err(|err| {
		error!("pinger task failed: {:?}", err);
		// do the same thing as above (error here, then return ())
	});

	rocket::tokio::try_join!(rocket, pinger).ok();
}
