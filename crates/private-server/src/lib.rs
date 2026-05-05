pub mod fns;
pub mod spa;
pub mod state;

pub fn routes(state: crate::state::AppState) -> commons_errors::Result<axum::routing::Router<()>> {
	use axum::routing::Router;

	Ok(Router::new()
		.nest(
			"/public",
			public_server::routes()
				.with_state(public_server::state::AppState::from_db(state.db.clone())?),
		)
		.merge(commons_servers::health::routes())
		.merge(fns::routes())
		.fallback(spa::handler)
		.with_state(state))
}
