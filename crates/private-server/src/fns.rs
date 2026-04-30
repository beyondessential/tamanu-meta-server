pub mod admins;
pub mod bestool;
pub mod commons;
pub mod devices;
pub mod servers;
pub mod sql;
pub mod statuses;
pub mod versions;

pub fn routes() -> axum::Router<crate::state::AppState> {
	use axum::Router;
	Router::new().nest(
		"/api",
		Router::new()
			.nest("/admins", admins::routes())
			.nest("/bestool", bestool::routes())
			.nest("/commons", commons::routes())
			.nest("/devices", devices::routes())
			.nest("/servers", servers::routes())
			.nest("/sql", sql::routes())
			.nest("/statuses", statuses::routes())
			.nest("/versions", versions::routes()),
	)
}
