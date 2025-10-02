#[cfg(feature = "ui")]
use axum::extract::State;
use axum::routing::Router;

use crate::state::AppState;

pub mod artifacts;
#[cfg(feature = "ui")]
pub mod password;
pub mod servers;
pub mod state;
pub mod statuses;
#[cfg(feature = "ui")]
pub mod timesync;
pub mod versions;

pub fn routes() -> Router<AppState> {
	#[cfg_attr(not(feature = "ui"), expect(unused_mut))]
	let mut router = Router::new()
		.nest("/artifacts", artifacts::routes())
		.nest("/servers", servers::routes())
		.nest("/status", statuses::routes())
		.nest("/versions", versions::routes());

	#[cfg(feature = "ui")]
	{
		use axum::routing::get;
		use tower_http::services::ServeDir;
		router = router
			.route("/", get(index))
			.route("/errors/{slug}", get(error))
			.merge(commons_servers::health::routes())
			.merge(timesync::routes())
			.merge(password::routes())
			.nest_service("/static", ServeDir::new("static"));
	}

	router
}

#[cfg(feature = "ui")]
async fn index(
	State(db): State<database::Db>,
	State(tera): State<std::sync::Arc<tera::Tera>>,
) -> commons_errors::Result<axum::response::Html<String>> {
	use database::versions::Version;
	use tera::Context;

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
	Ok(axum::response::Html(html))
}

#[cfg(feature = "ui")]
async fn error(axum::extract::Path(slug): axum::extract::Path<String>) -> axum::response::Redirect {
	axum::response::Redirect::temporary(&format!(
		"https://github.com/beyondessential/tamanu-meta-server/blob/{version}/ERRORS.md#{slug}",
		version = env!("CARGO_PKG_VERSION")
	))
}
