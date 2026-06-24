pub mod backup_probe;
pub mod fns;
pub mod openapi;
pub mod spa;
pub mod state;

pub fn routes(state: crate::state::AppState) -> commons_errors::Result<axum::routing::Router<()>> {
	use axum::middleware;
	use axum::routing::Router;
	use utoipa::OpenApi;
	use utoipa_axum::router::OpenApiRouter;
	use utoipa_swagger_ui::SwaggerUi;

	let (api_router, api_spec) = OpenApiRouter::with_openapi(openapi::ApiDoc::openapi())
		.merge(fns::routes())
		.split_for_parts();

	// `/public/...` accepts tagged-device callers via the dual-auth
	// device extractor. Everything else (admin API, Swagger, SPA) is
	// human-only — the tagged-device guard 403s those callers up front
	// rather than relying on downstream extractors' opportunistic checks.
	let non_public = Router::new()
		.merge(commons_servers::health::routes())
		.merge(api_router)
		.merge(SwaggerUi::new("/api/docs").url("/api/openapi.json", api_spec))
		.fallback(spa::handler)
		.layer(middleware::from_fn(
			commons_servers::tailnet_guard::reject_tagged_devices,
		));

	Ok(Router::new()
		.nest(
			"/public",
			Router::from(public_server::routes().with_state(
				// Wire the backup-credential clients (STS + kube Secret store) so the
				// nested public API can issue backup credentials and serve the repo
				// target/password, not just the DB-only endpoints.
				public_server::state::AppState::for_nested_mount(
					state.db.clone(),
					state.tailnet_directory.clone(),
					state.sts.clone(),
					state.kube.clone(),
				)?,
			)),
		)
		.merge(non_public)
		.with_state(state))
}
