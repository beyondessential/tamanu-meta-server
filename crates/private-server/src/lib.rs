pub mod fns;
pub mod openapi;
pub mod spa;
pub mod state;

pub fn routes(state: crate::state::AppState) -> commons_errors::Result<axum::routing::Router<()>> {
	use axum::routing::Router;
	use utoipa::OpenApi;
	use utoipa_axum::router::OpenApiRouter;
	use utoipa_swagger_ui::SwaggerUi;

	let (api_router, api_spec) = OpenApiRouter::with_openapi(openapi::ApiDoc::openapi())
		.merge(fns::routes())
		.split_for_parts();

	Ok(Router::new()
		.nest(
			"/public",
			public_server::routes()
				.with_state(public_server::state::AppState::from_db(state.db.clone())?),
		)
		.merge(commons_servers::health::routes())
		.merge(api_router)
		.merge(SwaggerUi::new("/api/docs").url("/api/openapi.json", api_spec))
		.fallback(spa::handler)
		.with_state(state))
}
