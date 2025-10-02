pub mod admins;
pub mod statuses;

#[cfg(feature = "ssr")]
pub fn routes() -> axum::Router<crate::state::AppState> {
	axum::Router::new().merge(statuses::routes())
}
