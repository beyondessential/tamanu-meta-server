pub mod admins;
pub mod bestool;
pub mod commons;
pub mod devices;
pub mod servers;
pub mod sql;
pub mod statuses;
pub mod versions;

#[cfg(feature = "ssr")]
pub fn routes() -> axum::Router<crate::state::AppState> {
	axum::Router::new()
}
