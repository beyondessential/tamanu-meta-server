use axum::routing::Router;
use rocket::{Build, Rocket, fs::FileServer};
use rocket_db_pools::Database as _;
use rocket_dyn_templates::Template;

use crate::{db::Db, state::AppState};

pub use super::health;

pub mod statuses;

#[catch(404)]
fn not_found() {}

pub fn rocket(prefix: String) -> Rocket<Build> {
	rocket::build()
		.attach(Template::fairing())
		.attach(Db::init())
		.register(format!("{prefix}/"), catchers![not_found])
		.mount(format!("{prefix}/static"), FileServer::from("static"))
}

pub fn routes(prefix: String) -> Router<AppState> {
	Router::new().nest(
		&format!("{prefix}/"),
		Router::new()
			.merge(health::routes())
			.merge(statuses::routes()),
	)
}
