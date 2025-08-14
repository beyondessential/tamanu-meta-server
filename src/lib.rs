#[macro_use]
extern crate rocket;

use std::net::SocketAddr;

use axum::Router;
pub use db::Db;
use rocket::figment::Figment;
use rocket_db_pools::Database;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

pub use servers::private::routes as private_routes;
// pub use servers::public::routes as public_routes;
use state::AppState;

pub(crate) mod db;
pub mod error;
pub mod pingtask;
pub(crate) mod schema;
pub(crate) mod servers;
pub(crate) mod state;
pub(crate) mod views;

pub async fn serve(routes: Router<AppState>, addr: SocketAddr) -> error::Result<()> {
	let service = routes
		.with_state(AppState::init()?)
		.layer(TraceLayer::new_for_http())
		.into_make_service();
	let listener = TcpListener::bind(addr).await?;
	tracing::debug!("listening on {}", listener.local_addr()?);
	axum::serve(listener, service).await?;
	Ok(())
}

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
