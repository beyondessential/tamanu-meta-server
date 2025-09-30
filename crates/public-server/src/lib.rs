use std::sync::Arc;

use axum::{
	extract::{Path, State},
	response::{Html, Redirect},
	routing::{Router, get},
};
use commons_errors::Result;
use commons_servers::health;
use database::{Db, versions::Version};
use tera::{Context, Tera};
use tower_http::services::ServeDir;

use crate::state::AppState;

pub mod artifacts;
pub mod password;
pub mod servers;
pub mod state;
pub mod statuses;
pub mod timesync;
pub mod versions;

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/", get(index))
		.route("/errors/{slug}", get(error))
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

async fn error(Path(slug): Path<String>) -> Redirect {
	Redirect::temporary(&format!(
		"https://github.com/beyondessential/tamanu-meta-server/blob/{version}/ERRORS.md#{slug}",
		version = env!("CARGO_PKG_VERSION")
	))
}
