use axum::{Json, extract::State};
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{backup_jobs::BillingLabels, device_auth::ServerDevice};
use commons_types::server::TagMap;
use database::{Db, server_groups::ServerGroup, servers::Server};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(get_self))
}

/// Get the tags for the calling device's own server.
///
/// Returns the effective set of tags for the server the calling device is
/// registered as: any tags set on the server itself, overlaid onto any tags
/// inherited from its server group (a tag set on the server takes precedence
/// over a group tag with the same key). If the server isn't in a group,
/// this returns just its own tags.
///
/// The result also includes a few read-only, synthetic tags describing the
/// server, under the reserved `canopy:` key prefix: `canopy:kind`,
/// `canopy:rank` (if the server has one set), and `canopy:group-id` /
/// `canopy:group-name` (if the server belongs to a group). Operators cannot
/// set tags under that prefix, so these never collide with tags you set
/// yourself.
///
/// When the server belongs to a group, the effective `billing.*` labels are
/// also included, matching the labels canopy attributes to cloud resources:
/// `billing.product`, `billing.deployment`, and `billing.stage` (the last
/// derived from *this* server's own rank, and omitted when the server has no
/// rank). The stage is per-server, not the group's highest rank, so a `clone`
/// server reports `billing.stage=clone` rather than the group's `prod`.
///
/// These are only defaults: a stored `billing.*` tag is honoured over the
/// computed value — the server's own tag first, then the group's. So an
/// operator can pin any billing label on a specific server or the whole group.
///
/// - **401**: the request has no client certificate, or the certificate
///   doesn't match a known device.
/// - **409**: the calling device is attached to more than one server, which
///   should not normally happen; contact support if you see this.
/// - **412**: the device is registered but has not yet been attached to a
///   server.
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
	let mut merged = server.tags_for_device(&mut conn).await?;

	// Fill in the group's effective billing labels where the server doesn't
	// already carry one, matching what canopy attributes to the group's cloud
	// resources. `merged` already holds server tags overlaid on group tags, so a
	// stored `billing.*` tag (server's own first, then the group's) is honoured
	// and only the missing labels fall back to computed values. `billing.stage`
	// is computed from *this* server's own rank, not the group's highest — a
	// rank=clone server must report `billing.stage=clone`, never the group's
	// `prod`, so its cost attributes to the right stage.
	if let Some(group_id) = server.group_id {
		let group = ServerGroup::get_by_id(&mut conn, group_id).await?;
		for (key, value) in
			BillingLabels::from_group(&group.tags, &group.name, server.rank).into_tags()
		{
			merged.0.entry(key).or_insert(value);
		}
	}

	Ok(Json(merged))
}
