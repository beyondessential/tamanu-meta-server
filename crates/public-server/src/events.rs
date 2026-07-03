use axum::{Json, extract::State};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::ServerDevice;
use database::{
	Db,
	issues::{Issue, NewEvent},
	servers::Server,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(create))
}

#[utoipa::path(
	post,
	path = "/events",
	operation_id = "submit_event",
	tag = "events",
	security(("server-device" = [])),
	request_body = NewEvent,
	responses(
		(status = 200, body = Issue),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
		(status = 412, description = "Device is not registered against any server.", body = ProblemDetailsSchema),
	),
)]
async fn create(
	State(db): State<Db>,
	device: ServerDevice,
	Json(event): Json<NewEvent>,
) -> Result<Json<Issue>> {
	if event.source.eq_ignore_ascii_case("manual") {
		return Err(AppError::SourceManualForbidden);
	}
	if event.r#ref.trim().is_empty() {
		return Err(AppError::custom("ref is required"));
	}

	let mut conn = db.get().await?;
	let device_id = device.0.0.id;

	// Strict: device must be registered against a server before reporting events.
	// (servers.device_id is unique, so at most one server matches.)
	let server = Server::get_by_device_id(&mut conn, device_id)
		.await?
		.into_iter()
		.next()
		.ok_or(AppError::DeviceHasNoServer)?;

	let issue = event.save(&mut conn, server.id, Some(device_id)).await?;
	Ok(Json(issue))
}
