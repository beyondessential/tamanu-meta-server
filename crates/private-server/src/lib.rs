pub mod app;

#[cfg(feature = "ssr")]
pub mod state;
#[cfg(feature = "ssr")]
pub mod statuses;

#[cfg(feature = "ssr")]
pub fn routes(prefix: String, state: crate::state::AppState) -> axum::routing::Router<()> {
	use axum::routing::Router;

	let prefix = format!("{prefix}/");

	Router::new()
		.nest(
			&prefix,
			Router::new()
				.merge(commons_servers::health::routes())
				.merge(statuses::routes())
				.with_state(state),
		)
		.nest(&prefix, {
			use leptos::config::get_configuration;
			use leptos_axum::{LeptosRoutes, generate_route_list};
			use tower_http::services::ServeDir;

			let conf = get_configuration(None).unwrap();
			let leptos_options = conf.leptos_options;
			let routes = generate_route_list(crate::app::App);

			Router::new()
				.leptos_routes(&leptos_options, routes, {
					let leptos_options = leptos_options.clone();
					move || crate::app::shell(leptos_options.clone())
				})
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
				.fallback(leptos_axum::file_and_error_handler(crate::app::shell))
				.with_state(leptos_options)
		})
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
	use crate::app::*;
	console_error_panic_hook::set_once();
	leptos::mount::hydrate_body(App);
}
