use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
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

#[derive(Deserialize, ToSchema)]
pub struct ServerScopeArgs {
	pub server_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct GroupScopeArgs {
	pub server_group_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct SilenceServerArgs {
	pub server_id: Uuid,
	pub source: String,
	#[serde(rename = "ref")]
	pub r#ref: String,
}

#[derive(Deserialize, ToSchema)]
pub struct SilenceGroupArgs {
	pub server_group_id: Uuid,
	pub source: String,
	#[serde(rename = "ref")]
	pub r#ref: String,
}

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
	Json(args): Json<ServerScopeArgs>,
) -> Result<Json<Vec<ServerSilencedRef>>> {
	let mut conn = state.db.get().await?;
	let rows = ServerSilencedRef::list_for_server(&mut conn, args.server_id).await?;
	Ok(Json(rows))
}

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
	Json(args): Json<GroupScopeArgs>,
) -> Result<Json<Vec<ServerGroupSilencedRef>>> {
	let mut conn = state.db.get().await?;
	let rows = ServerGroupSilencedRef::list_for_group(&mut conn, args.server_group_id).await?;
	Ok(Json(rows))
}

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
