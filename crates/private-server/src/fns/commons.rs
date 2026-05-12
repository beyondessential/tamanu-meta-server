use axum::Json;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(public_url))
		.routes(routes!(server_versions_url))
		.routes(routes!(is_current_user_admin))
}

#[utoipa::path(
	post,
	path = "/public_url",
	tag = "commons",
	responses(
		(status = 200, description = "Public-server URL, if configured.", body = Option<String>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn public_url() -> Result<Json<Option<String>>> {
	Ok(Json(std::env::var("PUBLIC_URL").ok()))
}

#[utoipa::path(
	post,
	path = "/server_versions_url",
	tag = "commons",
	responses(
		(status = 200, description = "Server-versions URL with embedded auth secret, if configured.", body = Option<String>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn server_versions_url() -> Result<Json<Option<String>>> {
	let url = (|| {
		let public_url = std::env::var("PUBLIC_URL").ok()?;
		let secret = std::env::var("SERVER_VERSIONS_SECRET").ok()?;
		Some(format!("{public_url}/server-versions?s={secret}"))
	})();
	Ok(Json(url))
}

// No `security` block: the handler intentionally accepts unauthenticated
// callers and reports `false`. Marking it admin-gated (or even user-gated)
// would make Swagger UI demand auth before letting you call it, defeating
// the point of the probe.
#[utoipa::path(
	post,
	path = "/is_current_user_admin",
	tag = "commons",
	responses(
		(status = 200, description = "`true` if the caller's Tailscale identity is on the admin allow-list; `false` otherwise (including when no Tailscale identity is present).", body = bool, content_type = "application/json"),
	),
)]
pub async fn is_current_user_admin(
	admin: std::result::Result<TailscaleAdmin, AppError>,
) -> Json<bool> {
	Json(admin.is_ok())
}
