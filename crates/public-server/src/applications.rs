use axum::{Json, extract::State};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::ServerDevice;

use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_types::server::{app_type::ApplicationType, rank::ServerRank};
use database::{Db, applications::Application, url_field::UrlField};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(self_identity))
}

/// A publicly-listed central server that a client can connect to.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicServer {
	/// Public-facing display name of the server.
	pub name: String,
	/// The server's reachable base URL.
	pub host: UrlField,
	/// The server's environment tier (production, clone, demo, test, or
	/// dev), if set. Used to order the listing and to let clients label
	/// non-production entries.
	pub rank: Option<ServerRank>,
}

fn rank_order(rank: &Option<ServerRank>) -> u32 {
	match rank {
		Some(ServerRank::Production) => 0,
		Some(ServerRank::Clone) => 1,
		Some(ServerRank::Demo) => 2,
		Some(ServerRank::Test) => 3,
		Some(ServerRank::Dev) => 4,
		_ => 5,
	}
}

/// List publicly-listed central applications.
///
/// Returns every central server that has both a public display name and a
/// reachable host configured, ordered by environment tier (production
/// first, then clone, demo, test, dev) and then by name. Used by clients
/// to let a user pick which server to connect to.
#[utoipa::path(
	get,
	path = "/",
	operation_id = "list_servers",
	tag = "applications",
	responses(
		(status = 200, description = "Publicly-listed central applications, ordered by rank then name.", body = Vec<PublicServer>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn list(State(db): State<Db>) -> Result<Json<Vec<PublicServer>>> {
	let mut db = db.get().await?;
	let mut applications =
		Application::list_by_type(&mut db, ApplicationType::TamanuCentral, 0, None)
			.await?
			.into_iter()
			.filter_map(|s| {
				// Only list applications that have both a public name and a URL — the
				// mobile app needs a reachable host.
				match (s.public_name, s.host) {
					(Some(name), Some(host)) => Some(PublicServer {
						name,
						host,
						rank: s.rank,
					}),
					_ => None,
				}
			})
			.collect::<Vec<_>>();

	applications.sort_by(|a, b| {
		rank_order(&a.rank)
			.cmp(&rank_order(&b.rank))
			.then_with(|| a.name.cmp(&b.name))
	});

	Ok(Json(applications))
}

/// The calling device's own identity, as assigned at enrollment.
#[derive(Debug, Serialize, ToSchema)]
pub struct SelfResponse {
	/// The server the calling device is enrolled as.
	pub server_id: Uuid,
	/// The calling device's own identity.
	pub device_id: Uuid,
}

/// Report the calling device's own identity.
///
/// Deprecated in favour of `GET /machines/self`. This asks which *application*
/// the caller is, which a box running more than one cannot answer; the machine
/// endpoint asks which box it is and answers for any of them.
///
/// Resolves the caller from its device certificate and returns the server
/// it is enrolled as together with its own device ID — the same pair
/// returned when the device completed enrollment. A device authenticates
/// entirely from its certificate, so it never needs these IDs to make
/// calls; this endpoint lets one that has lost track of them recover them.
///
/// - **401**: the request has no client certificate, or the certificate
///   doesn't match a known device.
/// - **409**: the calling device is attached to more than one server, which
///   should not normally happen; contact support if you see this.
/// - **412**: the device is registered but has not yet been attached to a
///   server.
// spec: DID
#[utoipa::path(
	get,
	path = "/self",
	operation_id = "server_self",
	tag = "applications",
	security(("server-device" = [])),
	responses(
		(status = 200, body = SelfResponse),
		(status = 401, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
		(status = 412, body = ProblemDetailsSchema),
	),
)]
pub async fn self_identity(
	device: ServerDevice,
	State(db): State<Db>,
) -> Result<Json<SelfResponse>> {
	let mut conn = db.get().await?;
	let device_id = device.0.0.id;
	let mut applications = Application::get_by_device_id(&mut conn, device_id).await?;
	if applications.len() > 1 {
		return Err(AppError::Conflict(format!(
			"device {device_id} is attached to {} applications; expected at most one",
			applications.len(),
		)));
	}
	let server = applications.pop().ok_or(AppError::DeviceHasNoServer)?;
	Ok(Json(SelfResponse {
		server_id: server.id,
		device_id,
	}))
}
