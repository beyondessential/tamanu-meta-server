use axum::routing::{Router, any};

pub fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
	Router::new()
		.route("/livez", any(async || {}))
		.route("/healthz", any(async || {}))
}
