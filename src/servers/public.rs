use std::sync::Arc;

use axum::{
	extract::State,
	response::Html,
	routing::{Router, get},
};
use rocket::{Build, Rocket};
use tera::{Context, Tera};
use tower_http::services::ServeDir;

use crate::{
	db::versions::Version,
	error::Result,
	state::{AppState, Db},
};

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
			versions::list,
			versions::create,
			versions::delete,
			versions::update_for,
			versions::get_artifacts,
			versions::view_artifacts,
			versions::view_mobile_install,
		],
	)
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/", get(index))
		.merge(health::routes())
		.merge(timesync::routes())
		.merge(password::routes())
		.nest("/artifacts", artifacts::routes())
		.nest("/servers", servers::routes())
		.nest("/status", statuses::routes())
		.nest("/versions", versions::routes())
		.nest_service("/static", ServeDir::new("static"))
}

async fn index(State(db): State<Db>, State(tera): State<Arc<Tera>>) -> Result<Html<String>> {
	let mut db = db.get().await?;
	let mut versions = Version::get_all(&mut db).await?;
	for version in &mut versions {
		version.changelog = versions::parse_markdown(&version.changelog);
	}
	let env = std::env::vars().collect::<std::collections::BTreeMap<String, String>>();
	let mut context = Context::new();
	context.insert("versions", &versions);
	context.insert("env", &env);
	let html = tera.render("versions", &context)?;
	Ok(Html(html))
}
