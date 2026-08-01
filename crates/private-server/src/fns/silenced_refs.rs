use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::{TailscaleAdmin, TailscaleUser};
use commons_types::Uuid;
use database::silenced_refs::{ServerGroupSilencedRef, ServerSilencedRef};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list_for_server))
		.routes(routes!(list_for_group))
		.routes(routes!(silence_server))
		.routes(routes!(unsilence_server))
		.routes(routes!(silence_group))
		.routes(routes!(unsilence_group))
}

/// Request body identifying a server to look up silences for.
#[derive(Deserialize, ToSchema)]
pub struct ServerScopeArgs {
	/// The server to look up silences for.
	pub server_id: Uuid,
}

/// Request body identifying a server group to look up silences for.
#[derive(Deserialize, ToSchema)]
pub struct GroupScopeArgs {
	/// The server group to look up silences for.
	pub server_group_id: Uuid,
}

/// Request body identifying an issue to silence (or unsilence) on a
/// single server.
#[derive(Deserialize, ToSchema)]
pub struct SilenceServerArgs {
	/// The server to silence the issue on.
	pub server_id: Uuid,
	/// Identifies what raises the issue being silenced — for example a
	/// specific healthcheck or backup pipeline.
	pub source: String,
	/// The specific issue identifier within `source` to silence.
	#[serde(rename = "ref")]
	pub r#ref: String,
}

/// Request body identifying an issue to silence (or unsilence) across a
/// whole server group.
#[derive(Deserialize, ToSchema)]
pub struct SilenceGroupArgs {
	/// The server group to silence the issue on.
	pub server_group_id: Uuid,
	/// Identifies what raises the issue being silenced — for example a
	/// specific healthcheck or backup pipeline.
	pub source: String,
	/// The specific issue identifier within `source` to silence.
	#[serde(rename = "ref")]
	pub r#ref: String,
}

/// List server-scoped silences for a server.
///
/// Returns every (source, ref) pair currently silenced specifically for
/// this server, most recently created first. Doesn't include silences
/// applied at the server's group level.
#[utoipa::path(
	post,
	path = "/list_for_server",
	tag = "silenced_refs",
	security(("tailscale-user" = [])),
	request_body = ServerScopeArgs,
	responses(
		(status = 200, body = Vec<ServerSilencedRef>),
	),
)]
pub async fn list_for_server(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ServerScopeArgs>,
) -> Result<Json<Vec<ServerSilencedRef>>> {
	let mut conn = state.db.get().await?;
	let rows = ServerSilencedRef::list_for_server(&mut conn, args.server_id).await?;
	Ok(Json(rows))
}

/// List group-scoped silences for a server group.
///
/// Returns every (source, ref) pair currently silenced for this server
/// group — applying to every server in the group — most recently created
/// first.
#[utoipa::path(
	post,
	path = "/list_for_group",
	tag = "silenced_refs",
	security(("tailscale-user" = [])),
	request_body = GroupScopeArgs,
	responses(
		(status = 200, body = Vec<ServerGroupSilencedRef>),
	),
)]
pub async fn list_for_group(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<GroupScopeArgs>,
) -> Result<Json<Vec<ServerGroupSilencedRef>>> {
	let mut conn = state.db.get().await?;
	let rows = ServerGroupSilencedRef::list_for_group(&mut conn, args.server_group_id).await?;
	Ok(Json(rows))
}

/// Silence an issue on a server.
///
/// Suppresses alerting for the given (source, ref) pair on this server:
/// matching issues keep being recorded, but stop counting toward opening
/// or extending an incident. Idempotent — silencing a pair that's already
/// silenced leaves the original entry, including who created it and when,
/// unchanged. Requires admin access. Returns 400 if the request is
/// invalid, for example if it references a server that doesn't exist.
#[utoipa::path(
	post,
	path = "/silence_server",
	tag = "silenced_refs",
	security(("tailscale-admin" = [])),
	request_body = SilenceServerArgs,
	responses(
		(status = 200, body = ServerSilencedRef),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn silence_server(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<SilenceServerArgs>,
) -> Result<Json<ServerSilencedRef>> {
	let mut conn = state.db.get().await?;
	let row = ServerSilencedRef::add(
		&mut conn,
		args.server_id,
		&args.source,
		&args.r#ref,
		Some(&admin.0.login),
	)
	.await?;
	Ok(Json(row))
}

/// Unsilence an issue on a server.
///
/// Removes a server-scoped silence for the given (source, ref) pair, if
/// one exists. Removing a silence that isn't there is not an error.
/// Requires admin access.
#[utoipa::path(
	post,
	path = "/unsilence_server",
	tag = "silenced_refs",
	security(("tailscale-admin" = [])),
	request_body = SilenceServerArgs,
	responses(
		(status = 200),
	),
)]
pub async fn unsilence_server(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SilenceServerArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	ServerSilencedRef::remove(&mut conn, args.server_id, &args.source, &args.r#ref).await?;
	Ok(Json(()))
}

/// Silence an issue on a server group.
///
/// Suppresses alerting for the given (source, ref) pair across every
/// server in this group: matching issues keep being recorded, but stop
/// counting toward opening or extending an incident. Idempotent —
/// silencing a pair that's already silenced leaves the original entry,
/// including who created it and when, unchanged. Requires admin access.
/// Returns 400 if the request is invalid, for example if it references a
/// server group that doesn't exist.
#[utoipa::path(
	post,
	path = "/silence_group",
	tag = "silenced_refs",
	security(("tailscale-admin" = [])),
	request_body = SilenceGroupArgs,
	responses(
		(status = 200, body = ServerGroupSilencedRef),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn silence_group(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<SilenceGroupArgs>,
) -> Result<Json<ServerGroupSilencedRef>> {
	let mut conn = state.db.get().await?;
	let row = ServerGroupSilencedRef::add(
		&mut conn,
		args.server_group_id,
		&args.source,
		&args.r#ref,
		Some(&admin.0.login),
	)
	.await?;
	Ok(Json(row))
}

/// Unsilence an issue on a server group.
///
/// Removes a group-scoped silence for the given (source, ref) pair, if
/// one exists. Removing a silence that isn't there is not an error.
/// Requires admin access.
#[utoipa::path(
	post,
	path = "/unsilence_group",
	tag = "silenced_refs",
	security(("tailscale-admin" = [])),
	request_body = SilenceGroupArgs,
	responses(
		(status = 200),
	),
)]
pub async fn unsilence_group(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SilenceGroupArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	ServerGroupSilencedRef::remove(&mut conn, args.server_group_id, &args.source, &args.r#ref)
		.await?;
	Ok(Json(()))
}
