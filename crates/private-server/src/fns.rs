use serde::{Deserialize, Serialize};
use utoipa_axum::router::OpenApiRouter;

pub mod admins;
pub mod backups;
pub mod bestool;
pub mod commons;
pub mod devices;
pub mod healthchecks;
pub mod incidents;
pub mod issues;
pub mod restore_replicas;
pub mod server_groups;
pub mod servers;
pub mod silenced_refs;
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
	OpenApiRouter::new().nest(
		"/api",
		OpenApiRouter::new()
			.nest("/admins", admins::routes())
			.nest("/backups", backups::routes())
			.nest("/bestool", bestool::routes())
			.nest("/commons", commons::routes())
			.nest("/devices", devices::routes())
			.nest("/healthchecks", healthchecks::routes())
			.nest("/incidents", incidents::routes())
			.nest("/issues", issues::routes())
			.nest("/restore_replicas", restore_replicas::routes())
			.nest("/server_groups", server_groups::routes())
			.nest("/servers", servers::routes())
			.nest("/silenced_refs", silenced_refs::routes())
			.nest("/sql", sql::routes())
			.nest("/statuses", statuses::routes())
			.nest("/versions", versions::routes()),
	)
}
