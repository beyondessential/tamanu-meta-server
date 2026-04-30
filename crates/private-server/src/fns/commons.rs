use axum::Json;
use axum::routing::{Router, post};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/public_url", post(public_url))
		.route("/server_versions_url", post(server_versions_url))
		.route("/is_current_user_admin", post(is_current_user_admin))
}

pub async fn public_url() -> Result<Json<Option<String>>> {
	Ok(Json(std::env::var("PUBLIC_URL").ok()))
}

pub async fn server_versions_url() -> Result<Json<Option<String>>> {
	let url = (|| {
		let public_url = std::env::var("PUBLIC_URL").ok()?;
		let secret = std::env::var("SERVER_VERSIONS_SECRET").ok()?;
		Some(format!("{public_url}/server-versions?s={secret}"))
	})();
	Ok(Json(url))
}

pub async fn is_current_user_admin(
	admin: std::result::Result<TailscaleAdmin, AppError>,
) -> Json<bool> {
	Json(admin.is_ok())
}
