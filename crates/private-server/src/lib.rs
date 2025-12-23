#![recursion_limit = "256"]

pub mod app;
pub mod components;
pub mod fns;
#[cfg(feature = "ssr")]
pub mod state;

#[cfg(feature = "ssr")]
pub fn routes(state: crate::state::AppState) -> commons_errors::Result<axum::routing::Router<()>> {
	use axum::{
		extract::ConnectInfo,
		http::Request,
		middleware::{self, Next},
		routing::Router,
	};
	use leptos::prelude::provide_context;
	use leptos_axum::{LeptosRoutes as _, generate_route_list};
	use std::net::SocketAddr;
	use tower_http::services::ServeDir;

	// Middleware that redirects to /public routes if Host matches PUBLIC_URL
	let public_url_middleware = |state: crate::state::AppState| {
		middleware::from_fn(
			move |ConnectInfo(_addr): ConnectInfo<SocketAddr>,
			      req: Request<axum::body::Body>,
			      next: Next| {
				let _state = state.clone();
				async move {
					let host = req
						.headers()
						.get("host")
						.and_then(|h| h.to_str().ok())
						.unwrap_or("");

					// Check if this is a public URL access
					if let Ok(public_url) = std::env::var("PUBLIC_URL")
						&& let Ok(public_uri) = public_url.parse::<axum::http::Uri>()
						&& let Some(public_host) = public_uri.host()
						&& host.starts_with(public_host)
					{
						// Redirect to /public routes
						let path = req.uri().path().to_string();
						if !path.starts_with("/public") && path != "/" {
							// For non-root paths, prepend /public
							let new_path = format!("/public{}", path);
							let mut new_req = req;
							*new_req.uri_mut() = new_path.parse().unwrap();
							return next.run(new_req).await;
						} else if path == "/" {
							// For root path, rewrite to /public and let router handle it
							let mut new_req = req;
							*new_req.uri_mut() = "/public".parse().unwrap();
							return next.run(new_req).await;
						}
					}

					next.run(req).await
				}
			},
		)
	};

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
		.layer(public_url_middleware(state.clone()))
		.with_state(state))
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
	console_error_panic_hook::set_once();
	leptos::mount::hydrate_body(app::App);
}
