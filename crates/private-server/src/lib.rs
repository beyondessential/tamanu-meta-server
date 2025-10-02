#![recursion_limit = "256"]

pub mod app;

pub mod fns;
#[cfg(feature = "ssr")]
pub mod state;

#[cfg(feature = "ssr")]
pub fn routes(state: crate::state::AppState) -> commons_errors::Result<axum::routing::Router<()>> {
	use axum::routing::Router;
	use leptos::prelude::provide_context;
	use leptos_axum::{LeptosRoutes as _, generate_route_list};
	use tower_http::services::ServeDir;

	Ok(Router::new()
		.nest(
			"/public",
			public_server::routes()
				.with_state(public_server::state::AppState::from_db(state.db.clone())?),
		)
		.merge(commons_servers::health::routes())
		.merge(fns::routes())
		.nest_service(
			"/static",
			ServeDir::new("target/site/private")
				.precompressed_br()
				.precompressed_gzip()
				.fallback(
					ServeDir::new("target/site")
						.precompressed_br()
						.precompressed_gzip(),
				),
		)
		.nest_service(
			"/pkg",
			ServeDir::new("target/site/pkg")
				.precompressed_br()
				.precompressed_gzip(),
		)
		// .fallback(leptos_axum::file_and_error_handler(crate::app::shell))
		.leptos_routes_with_context(
			&state,
			generate_route_list(crate::app::App),
			{
				let state = state.clone();
				move || provide_context(state.clone())
			},
			{
				let state = state.clone();
				move || {
					crate::app::shell({
						let state = state.clone();
						state.leptos_options
					})
				}
			},
		)
		.with_state(state))
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
	console_error_panic_hook::set_once();
	leptos::mount::hydrate_islands();
}
