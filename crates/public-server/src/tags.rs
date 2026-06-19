use axum::{Json, extract::State};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::ServerDevice;
use commons_types::server::TagMap;
use database::{Db, servers::Server};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(get_self))
}

/// Merged tags for the calling device's server.
///
/// The device authenticates with its certificate; we resolve the (unique)
/// server backed by that device, then overlay the server's `tags` onto the
/// group's `tags` (server wins on key collision). For ungrouped servers,
/// returns just the server's own tags.
///
/// On top of the stored tags we inject synthetic, read-only tags describing
/// the server under the reserved `canopy:` namespace: `canopy:kind`,
/// `canopy:rank` (when set), and `canopy:group-id` / `canopy:group-name`
/// (when grouped). Operator-set tags can't use that prefix, so they never
/// collide. See [`Server::tags_for_device`].
///
/// - **401**: no certificate / cert doesn't match a known device.
/// - **412**: device is registered but isn't attached to a server yet.
/// - **409**: device is somehow attached to multiple servers (shouldn't
///   happen — the `servers.device_id` unique index guarantees this can't
///   occur, but the handler still surfaces the case rather than silently
///   picking one).
#[utoipa::path(
	get,
	path = "/",
	tag = "tags",
	security(("server-device" = [])),
	responses(
		(status = 200, body = TagMap),
		(status = 401, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
		(status = 412, body = ProblemDetailsSchema),
	),
)]
pub async fn get_self(device: ServerDevice, State(db): State<Db>) -> Result<Json<TagMap>> {
	let mut conn = db.get().await?;
	let device_id = device.0.0.id;
	let mut servers = Server::get_by_device_id(&mut conn, device_id).await?;
	if servers.len() > 1 {
		return Err(AppError::Conflict(format!(
			"device {device_id} is attached to {} servers; expected at most one",
			servers.len(),
		)));
	}
	let server = servers.pop().ok_or(AppError::DeviceHasNoServer)?;
	let merged = server.tags_for_device(&mut conn).await?;
	Ok(Json(merged))
}
