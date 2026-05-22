use axum::Json;
use axum::extract::State;
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{Uuid, server::TagMap};
use database::server_groups::{NewServerGroup, PartialServerGroup, ServerGroup};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(get))
		.routes(routes!(create))
		.routes(routes!(update))
		.routes(routes!(delete))
		.routes(routes!(search))
}

#[utoipa::path(
	post,
	path = "/list",
	operation_id = "server_groups_list",
	tag = "server_groups",
	security(("tailscale-user" = [])),
	responses(
		(status = 200, body = Vec<ServerGroup>),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_body: Json<serde_json::Value>,
) -> Result<Json<Vec<ServerGroup>>> {
	let mut conn = state.db.get().await?;
	let groups = ServerGroup::list_all(&mut conn).await?;
	Ok(Json(groups))
}

#[derive(Deserialize, ToSchema)]
pub struct GroupIdArgs {
	pub server_group_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GroupDetail {
	pub group: ServerGroup,
	pub servers: Vec<super::servers::ServerInfo>,
}

#[utoipa::path(
	post,
	path = "/get",
	operation_id = "server_groups_get",
	tag = "server_groups",
	security(("tailscale-user" = [])),
	request_body = GroupIdArgs,
	responses(
		(status = 200, body = GroupDetail),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get(
	State(state): State<AppState>,
	Json(args): Json<GroupIdArgs>,
) -> Result<Json<GroupDetail>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;
	let members = group.list_servers(&mut conn).await?;
	let group_name = group.name.clone();
	let mut servers: Vec<super::servers::ServerInfo> = members
		.into_iter()
		.map(|s| {
			let mut info = super::servers::server_to_info(s);
			info.group_name = Some(group_name.clone());
			info
		})
		.collect();
	servers.sort_by(|a, b| {
		a.name
			.as_deref()
			.unwrap_or("")
			.cmp(b.name.as_deref().unwrap_or(""))
	});
	Ok(Json(GroupDetail { group, servers }))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateArgs {
	pub name: String,
	#[serde(default)]
	pub notes: String,
	#[serde(default)]
	pub tags: TagMap,
}

#[utoipa::path(
	post,
	path = "/create",
	operation_id = "server_groups_create",
	tag = "server_groups",
	security(("tailscale-admin" = [])),
	request_body = CreateArgs,
	responses(
		(status = 200, body = ServerGroup),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<CreateArgs>,
) -> Result<Json<ServerGroup>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::create(
		&mut conn,
		NewServerGroup {
			name: args.name,
			notes: args.notes,
			tags: args.tags,
		},
	)
	.await?;
	Ok(Json(group))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateArgs {
	pub server_group_id: Uuid,
	pub data: PartialServerGroup,
}

#[utoipa::path(
	post,
	path = "/update",
	operation_id = "server_groups_update",
	tag = "server_groups",
	security(("tailscale-admin" = [])),
	request_body = UpdateArgs,
	responses(
		(status = 200, body = ServerGroup),
		(status = 400, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<UpdateArgs>,
) -> Result<Json<ServerGroup>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::update(&mut conn, args.server_group_id, args.data).await?;
	Ok(Json(group))
}

#[utoipa::path(
	post,
	path = "/delete",
	operation_id = "server_groups_delete",
	tag = "server_groups",
	security(("tailscale-admin" = [])),
	request_body = GroupIdArgs,
	responses(
		(status = 200),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn delete(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<GroupIdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	ServerGroup::delete(&mut conn, args.server_group_id).await?;
	Ok(Json(()))
}

#[derive(Deserialize, ToSchema)]
pub struct SearchArgs {
	pub query: String,
}

#[utoipa::path(
	post,
	path = "/search",
	operation_id = "server_groups_search",
	tag = "server_groups",
	security(("tailscale-user" = [])),
	request_body = SearchArgs,
	responses(
		(status = 200, body = Vec<ServerGroup>),
	),
)]
pub async fn search(
	State(state): State<AppState>,
	Json(args): Json<SearchArgs>,
) -> Result<Json<Vec<ServerGroup>>> {
	let mut conn = state.db.get().await?;
	let groups = ServerGroup::search(&mut conn, &args.query).await?;
	Ok(Json(groups))
}
