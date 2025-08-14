use axum::routing::Router;
use rocket::{Build, Rocket};
use tower_http::services::ServeDir;

use crate::{servers::version::Version, state::AppState};

pub use super::health;

pub mod artifacts;
pub mod password;
pub mod servers;
pub mod statuses;
pub mod timesync;
pub mod versions;

pub fn rocket() -> Rocket<Build> {
	rocket::build().mount(
		"/",
		routes![
			servers::list,
			servers::create,
			servers::edit,
			servers::delete,
			versions::list,
			versions::view,
			versions::create,
			versions::delete,
			versions::update_for,
			versions::get_artifacts,
			versions::view_artifacts,
			versions::view_mobile_install,
			artifacts::create,
		],
	)
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.merge(health::routes())
		.merge(timesync::routes())
		.merge(password::routes())
		.nest("/status", statuses::routes())
		.nest_service("/static", ServeDir::new("static"))
}
