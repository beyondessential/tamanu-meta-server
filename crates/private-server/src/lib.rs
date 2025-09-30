pub mod app;

#[cfg(feature = "ssr")]
pub mod state;
pub mod statuses;

#[cfg(feature = "ssr")]
pub fn routes(prefix: String, state: crate::state::AppState) -> axum::routing::Router<()> {
	use axum::routing::Router;
	use leptos::prelude::provide_context;
	use leptos_axum::{AxumRouteListing, LeptosRoutes as _, generate_route_list};
	use tower_http::services::ServeDir;

	let routes = generate_route_list(crate::app::App)
		.into_iter()
		.map(|route| {
			AxumRouteListing::new(
				format!("{prefix}{}", route.path()),
				route.mode().clone(),
				route.methods(),
				Vec::new(),
			)
		})
		.collect();

	Router::new()
		.nest(
			&format!("{prefix}/"),
			Router::new()
				.merge(commons_servers::health::routes())
				.merge(statuses::routes())
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
				), // .fallback(leptos_axum::file_and_error_handler(crate::app::shell))
		)
		.leptos_routes_with_context(
			&state,
			routes,
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
		.with_state(state)
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
	console_error_panic_hook::set_once();
	leptos::mount::hydrate_islands();
}
