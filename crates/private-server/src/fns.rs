use serde::{Deserialize, Serialize};
use utoipa_axum::router::OpenApiRouter;

pub mod admins;
pub mod bestool;
pub mod commons;
pub mod devices;
pub mod incidents;
pub mod issues;
pub mod servers;
pub mod sql;
pub mod statuses;
pub mod versions;

/// Standard wrapper for paginated list responses. The total reflects the full
/// row count (not just the current page) so the frontend can render page
/// counts without a separate count fetch.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Page<T> {
	pub items: Vec<T>,
	pub total: u64,
}

pub fn routes() -> OpenApiRouter<crate::state::AppState> {
	type Api = OpenApiRouter<crate::state::AppState>;
	OpenApiRouter::new().nest(
		"/api",
		OpenApiRouter::new()
			.nest("/admins", admins::routes())
			.nest("/bestool", Api::from(bestool::routes()))
			.nest("/commons", Api::from(commons::routes()))
			.nest("/devices", Api::from(devices::routes()))
			.nest("/incidents", Api::from(incidents::routes()))
			.nest("/issues", Api::from(issues::routes()))
			.nest("/servers", Api::from(servers::routes()))
			.nest("/sql", Api::from(sql::routes()))
			.nest("/statuses", Api::from(statuses::routes()))
			.nest("/versions", Api::from(versions::routes())),
	)
}
