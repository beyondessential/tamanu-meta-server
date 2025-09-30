use axum::routing::Router;
use tower_http::services::ServeDir;

use crate::state::AppState;

pub use super::health;

pub mod statuses;

pub fn routes(prefix: String) -> Router<AppState> {
	Router::new().nest(
		&format!("{prefix}/"),
		Router::new()
			.merge(health::routes())
			.merge(statuses::routes())
			.nest_service("/static", ServeDir::new("static"))
			.fallback_service(ServeDir::new("web/private/dist")),
	)
}
