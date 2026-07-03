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

/// List all live server groups.
///
/// Returns every non-archived server group, including its name, notes, tags,
/// Slack notification delay, and effective version information. The request
/// body is ignored; send an empty JSON object.
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

/// The number of live servers in one server group.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GroupServerCount {
	/// Identifier of the server group.
	pub server_group_id: Uuid,
	/// Number of live (non-archived) servers currently in the group.
	pub server_count: i64,
}

/// Count live servers per group.
///
/// Returns one entry per server group that has at least one live
/// (non-archived) member server. Groups with no live members are omitted, so
/// treat a missing entry as a count of zero. The request body is ignored;
/// send an empty JSON object.
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

/// Identifies the server group to operate on.
#[derive(Deserialize, ToSchema)]
pub struct GroupIdArgs {
	/// Identifier of the server group.
	pub server_group_id: Uuid,
}

/// One effective billing label attributed to a group's cloud resources.
///
/// Labels are computed from the group's configuration: explicit `billing.*`
/// tags on the group are honoured verbatim; otherwise the product defaults to
/// `tamanu`, the deployment to the group name in lower-kebab-case, and the
/// stage to the group's highest-ranked live member (for example `prod`). The
/// stage label is omitted entirely when the group has no ranked members.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BillingTag {
	/// Label key, for example `billing.product`.
	pub key: String,
	/// Label value.
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

/// A server group together with its member servers and billing labels.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GroupDetail {
	/// The group itself.
	pub group: ServerGroup,
	/// The group's member servers, sorted by name, with current status and
	/// display host included.
	pub servers: Vec<super::servers::ServerInfo>,
	/// The group's effective `billing.*` labels (product/deployment/stage).
	pub billing_labels: Vec<BillingTag>,
}

/// Get a server group with its members.
///
/// Returns the group, its member servers (sorted by name, with current status
/// and display host), and the group's effective billing labels. Responds 404
/// if no group exists with the given identifier.
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

/// Request to create a new server group.
#[derive(Deserialize, ToSchema)]
pub struct ServerGroupsCreateArgs {
	/// Display name for the new group.
	pub name: String,
	/// Free-form notes about the group. Defaults to empty.
	#[serde(default)]
	pub notes: String,
	/// Tags to set on the group, as an object of string keys to string
	/// values. Keys with the reserved `canopy:` prefix cannot be set.
	/// Defaults to empty.
	#[serde(default)]
	pub tags: TagMap,
	/// Optional initial delay, in whole seconds, before an "incident opened"
	/// Slack notification for this group is delivered; an incident that
	/// resolves within the window never notifies. Omit to accept the default.
	#[serde(default)]
	#[schema(value_type = Option<i64>, format = "int64")]
	pub slack_open_delay: Option<database::pg_duration::PgDuration>,
}

/// Create a server group.
///
/// Creates a new, empty server group and returns it. Requires the caller to
/// be on the admin allow-list. Responds 400 if the request is invalid.
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

/// Request to update a server group.
#[derive(Deserialize, ToSchema)]
pub struct ServerGroupsUpdateArgs {
	/// Identifier of the group to update.
	pub server_group_id: Uuid,
	/// The fields to change. Any field omitted is left unchanged.
	pub data: PartialServerGroup,
}

/// Update a server group.
///
/// Applies a partial update: only the fields present in `data` (name, notes,
/// tags, Slack notification delay) are changed. Returns the updated group.
/// Requires the caller to be on the admin allow-list. Responds 404 if the
/// group does not exist and 400 if the request is invalid.
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

/// Archive a server group.
///
/// Soft-deletes the group: it disappears from live listings but is kept and
/// can be restored later. Requires the caller to be on the admin allow-list.
/// Responds 409 if the group still has live member servers; move or archive
/// those first.
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

/// Restore an archived server group.
///
/// Un-archives a previously deleted group so it reappears in live listings.
/// Requires the caller to be on the admin allow-list. Responds 404 if the
/// group does not exist.
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

/// List archived server groups.
///
/// Returns every group that has been archived (soft-deleted) and can be
/// restored. The request body is ignored; send an empty JSON object.
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

/// Search terms for finding server groups.
#[derive(Deserialize, ToSchema)]
pub struct ServerGroupsSearchArgs {
	/// Free-text search query.
	pub query: String,
}

/// Search live server groups.
///
/// Returns non-archived groups matching the free-text query.
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
