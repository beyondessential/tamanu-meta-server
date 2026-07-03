use axum::Json;
use axum::extract::State;
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::{backup_jobs::BillingLabels, tailscale_auth::TailscaleAdmin};
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
		.routes(routes!(restore))
		.routes(routes!(list_archived))
		.routes(routes!(server_counts))
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

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GroupServerCount {
	pub server_group_id: Uuid,
	pub server_count: i64,
}

/// Live (non-archived) server count per group, for the groups list. Groups with
/// no live members are omitted (the client defaults missing entries to 0).
#[utoipa::path(
	post,
	path = "/server_counts",
	operation_id = "server_groups_server_counts",
	tag = "server_groups",
	security(("tailscale-user" = [])),
	responses(
		(status = 200, body = Vec<GroupServerCount>),
	),
)]
pub async fn server_counts(
	State(state): State<AppState>,
	_body: Json<serde_json::Value>,
) -> Result<Json<Vec<GroupServerCount>>> {
	let mut conn = state.db.get().await?;
	let counts = ServerGroup::live_server_counts(&mut conn).await?;
	Ok(Json(
		counts
			.into_iter()
			.map(|(server_group_id, server_count)| GroupServerCount {
				server_group_id,
				server_count,
			})
			.collect(),
	))
}

#[derive(Deserialize, ToSchema)]
pub struct GroupIdArgs {
	pub server_group_id: Uuid,
}

/// One effective `billing.*` label canopy attributes a group's AWS resources
/// under (computed: explicit `billing.*` group tags honored verbatim, else
/// product `tamanu`, deployment = lower-kebab group name, stage = highest rank).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BillingTag {
	pub key: String,
	pub value: String,
}

/// A group's effective billing labels, for display on the group + server views.
pub(crate) async fn group_billing_labels(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group: &ServerGroup,
) -> Result<Vec<BillingTag>> {
	let highest_rank = ServerGroup::highest_member_ranks(conn, &[group.id])
		.await?
		.get(&group.id)
		.copied();
	Ok(
		BillingLabels::from_group(&group.tags, &group.name, highest_rank)
			.into_tags()
			.into_iter()
			.map(|(key, value)| BillingTag { key, value })
			.collect(),
	)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GroupDetail {
	pub group: ServerGroup,
	pub servers: Vec<super::servers::ServerInfo>,
	/// The group's effective `billing.*` labels (product/deployment/stage).
	pub billing_labels: Vec<BillingTag>,
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
	super::servers::decorate_with_status(&mut conn, &mut servers).await?;
	super::servers::fill_display_hosts(&mut conn, &mut servers).await?;
	let billing_labels = group_billing_labels(&mut conn, &group).await?;
	Ok(Json(GroupDetail {
		group,
		servers,
		billing_labels,
	}))
}

#[derive(Deserialize, ToSchema)]
pub struct ServerGroupsCreateArgs {
	pub name: String,
	#[serde(default)]
	pub notes: String,
	#[serde(default)]
	pub tags: TagMap,
	/// Optional initial value (seconds) for the group's Slack open
	/// cooldown. Omit to let the database default apply.
	#[serde(default)]
	#[schema(value_type = Option<i64>, format = "int64")]
	pub slack_open_delay: Option<database::pg_duration::PgDuration>,
}

#[utoipa::path(
	post,
	path = "/create",
	operation_id = "server_groups_create",
	tag = "server_groups",
	security(("tailscale-admin" = [])),
	request_body = ServerGroupsCreateArgs,
	responses(
		(status = 200, body = ServerGroup),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerGroupsCreateArgs>,
) -> Result<Json<ServerGroup>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::create(
		&mut conn,
		NewServerGroup {
			name: args.name,
			notes: args.notes,
			tags: args.tags,
			slack_open_delay: args.slack_open_delay,
		},
	)
	.await?;
	Ok(Json(group))
}

#[derive(Deserialize, ToSchema)]
pub struct ServerGroupsUpdateArgs {
	pub server_group_id: Uuid,
	pub data: PartialServerGroup,
}

#[utoipa::path(
	post,
	path = "/update",
	operation_id = "server_groups_update",
	tag = "server_groups",
	security(("tailscale-admin" = [])),
	request_body = ServerGroupsUpdateArgs,
	responses(
		(status = 200, body = ServerGroup),
		(status = 400, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerGroupsUpdateArgs>,
) -> Result<Json<ServerGroup>> {
	let mut conn = state.db.get().await?;
	let group = ServerGroup::update(&mut conn, args.server_group_id, args.data).await?;
	Ok(Json(group))
}

/// Archive (soft-delete) a group. Kept at `/delete` for the existing client;
/// the group is hidden from live listings but restorable. Refuses if the group
/// still has live members (409).
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
	ServerGroup::soft_delete(&mut conn, args.server_group_id).await?;
	Ok(Json(()))
}

#[utoipa::path(
	post,
	path = "/restore",
	operation_id = "server_groups_restore",
	tag = "server_groups",
	security(("tailscale-admin" = [])),
	request_body = GroupIdArgs,
	responses(
		(status = 200),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn restore(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<GroupIdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	ServerGroup::restore(&mut conn, args.server_group_id).await?;
	Ok(Json(()))
}

#[utoipa::path(
	post,
	path = "/list_archived",
	operation_id = "server_groups_list_archived",
	tag = "server_groups",
	security(("tailscale-user" = [])),
	responses(
		(status = 200, body = Vec<ServerGroup>),
	),
)]
pub async fn list_archived(
	State(state): State<AppState>,
	_body: Json<serde_json::Value>,
) -> Result<Json<Vec<ServerGroup>>> {
	let mut conn = state.db.get().await?;
	let groups = ServerGroup::list_archived(&mut conn).await?;
	Ok(Json(groups))
}

#[derive(Deserialize, ToSchema)]
pub struct ServerGroupsSearchArgs {
	pub query: String,
}

#[utoipa::path(
	post,
	path = "/search",
	operation_id = "server_groups_search",
	tag = "server_groups",
	security(("tailscale-user" = [])),
	request_body = ServerGroupsSearchArgs,
	responses(
		(status = 200, body = Vec<ServerGroup>),
	),
)]
pub async fn search(
	State(state): State<AppState>,
	Json(args): Json<ServerGroupsSearchArgs>,
) -> Result<Json<Vec<ServerGroup>>> {
	let mut conn = state.db.get().await?;
	let groups = ServerGroup::search(&mut conn, &args.query).await?;
	Ok(Json(groups))
}
