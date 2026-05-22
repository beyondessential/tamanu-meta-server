use axum::Json;
use axum::extract::State;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::server::CanopyTicket;
use commons_types::{
	Uuid,
	geo::GeoPoint,
	server::{TagMap, kind::ServerKind, rank::ServerRank},
	status::{HealthState, ShortStatus},
	version::VersionStr,
};
use database::{
	devices::{Device, DeviceConnection},
	server_groups::ServerGroup,
	servers::{PartialServer, Server},
	statuses::Status,
	url_field::UrlField,
	versions::Version,
};
use futures::future::join;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::fns::Page;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerDetailData {
	pub server: ServerInfo,
	pub device_info: Option<super::devices::DeviceInfo>,
	pub last_status: Option<ServerLastStatusData>,
	pub up: ShortStatus,
	pub health: HealthState,
	/// The group this server belongs to, with its notes/tags so the UI can
	/// render the "Group" section without a second fetch. `None` when the
	/// server is ungrouped.
	pub group: Option<ServerGroup>,
	/// Other servers in the same group (excluding `server`). Empty when the
	/// server is ungrouped or alone in its group.
	pub siblings: Vec<(ShortStatus, HealthState, ServerInfo)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerInfo {
	pub id: Uuid,
	pub name: Option<String>,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
	pub host: String,
	pub device_id: Option<Uuid>,
	pub group_id: Option<Uuid>,
	/// Display name of the group this server belongs to (denormalised so list
	/// rows don't need to fetch the group separately). `None` if ungrouped.
	pub group_name: Option<String>,
	pub listed: bool,
	pub cloud: Option<bool>,
	pub geolocation: Option<GeoPoint>,
	/// Downtime threshold in seconds before the reachability sweep files an
	/// issue. `0` (or any non-positive value) disables alerting for this
	/// server; the default at creation is 600 (10 minutes).
	pub alert_when_down_for: i64,
	pub notes: String,
	pub tags: TagMap,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerLastStatusData {
	pub id: Uuid,
	pub created_at: Timestamp,
	pub version: Option<VersionStr>,
	pub version_distance: Option<u64>,
	pub min_chrome_version: Option<u32>,
	pub platform: Option<String>,
	pub postgres: Option<String>,
	pub nodejs: Option<String>,
	pub timezone: Option<String>,
	/// Server's overall self-reported health from this status push.
	/// `true` for legacy rows that predate the contract.
	pub healthy: bool,
	/// Per-check breakdown from this push. `[]` for legacy rows.
	pub health: JsonValue,
	pub extra: JsonValue,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ServerDataUpdate {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub kind: Option<ServerKind>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub rank: Option<ServerRank>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub host: Option<String>,
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub device_id: Option<Option<Uuid>>,
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub group_id: Option<Option<Uuid>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub listed: Option<bool>,
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub cloud: Option<Option<bool>>,
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub geolocation: Option<Option<GeoPoint>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub alert_when_down_for: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub notes: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tags: Option<TagMap>,
}

fn deserialize_some<'de, T, D>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
	T: Deserialize<'de>,
	D: serde::Deserializer<'de>,
{
	Deserialize::deserialize(deserializer).map(Some)
}

pub(super) fn server_to_info(s: Server) -> ServerInfo {
	ServerInfo {
		id: s.id,
		name: s.name,
		kind: s.kind,
		rank: s.rank,
		host: s.host.0.to_string(),
		device_id: s.device_id,
		group_id: s.group_id,
		group_name: None,
		listed: s.listed,
		cloud: s.cloud,
		geolocation: s.geolocation,
		alert_when_down_for: s.alert_when_down_for.0.as_secs(),
		notes: s.notes,
		tags: s.tags,
	}
}

/// Like [`server_to_info`] but populates `group_name` by looking it up from
/// the supplied map (caller pre-fetches the relevant groups in one batch).
fn server_to_info_with_group(
	s: Server,
	group_names: &std::collections::HashMap<Uuid, String>,
) -> ServerInfo {
	let group_name = s.group_id.and_then(|gid| group_names.get(&gid).cloned());
	let mut info = server_to_info(s);
	info.group_name = group_name;
	info
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list_some))
		.routes(routes!(list_ungrouped))
		.routes(routes!(get_name))
		.routes(routes!(get_info))
		.routes(routes!(get_detail))
		.routes(routes!(update))
		.routes(routes!(import_ticket))
		.routes(routes!(attach_tailscale_device))
}

#[derive(Deserialize, ToSchema)]
pub struct ListArgs {
	pub kind: Option<ServerKind>,
	pub offset: u64,
	pub limit: Option<u64>,
}

#[utoipa::path(
	post,
	path = "/list_some",
	tag = "servers",
	request_body = ListArgs,
	responses(
		(status = 200, body = Page<ServerInfo>),
	),
)]
pub async fn list_some(
	State(state): State<AppState>,
	Json(args): Json<ListArgs>,
) -> Result<Json<Page<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let total = if let Some(kind) = args.kind {
		Server::count_by_kind(&mut conn, kind).await?
	} else {
		Server::count_all(&mut conn).await?
	};
	let servers = if let Some(kind) = args.kind {
		Server::list_by_kind(&mut conn, kind, args.offset, args.limit).await?
	} else {
		Server::get_all(&mut conn, args.offset, args.limit).await?
	};
	let group_names = collect_group_names(&mut conn, &servers).await?;
	let items = servers
		.into_iter()
		.map(|s| server_to_info_with_group(s, &group_names))
		.collect();
	Ok(Json(Page { items, total }))
}

/// Servers without a group, used by the Ungrouped tab. Returned alongside a
/// total count so the UI can show "(N ungrouped)" without a second fetch.
#[utoipa::path(
	post,
	path = "/list_ungrouped",
	tag = "servers",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, body = Page<ServerInfo>),
	),
)]
pub async fn list_ungrouped(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	_body: Json<serde_json::Value>,
) -> Result<Json<Page<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let total = Server::count_ungrouped(&mut conn).await?;
	let servers = Server::list_ungrouped(&mut conn).await?;
	let items = servers.into_iter().map(server_to_info).collect();
	Ok(Json(Page { items, total }))
}

#[derive(Deserialize, ToSchema)]
pub struct ServerIdArgs {
	pub server_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/get_name",
	tag = "servers",
	request_body = ServerIdArgs,
	responses(
		(status = 200, body = String, content_type = "application/json"),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get_name(
	State(state): State<AppState>,
	Json(args): Json<ServerIdArgs>,
) -> Result<Json<String>> {
	let mut conn = state.db.get().await?;
	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	Ok(Json(
		server.name.unwrap_or_else(|| server.host.0.to_string()),
	))
}

#[utoipa::path(
	post,
	path = "/get_info",
	tag = "servers",
	request_body = ServerIdArgs,
	responses(
		(status = 200, body = ServerInfo),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get_info(
	State(state): State<AppState>,
	Json(args): Json<ServerIdArgs>,
) -> Result<Json<ServerInfo>> {
	let db = state.db;
	let mut conn = db.get().await?;
	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	let group_name = if let Some(gid) = server.group_id {
		Some(ServerGroup::get_by_id(&mut conn, gid).await?.name)
	} else {
		None
	};
	let mut info = server_to_info(server);
	info.group_name = group_name;
	Ok(Json(info))
}

#[utoipa::path(
	post,
	path = "/get_detail",
	tag = "servers",
	request_body = ServerIdArgs,
	responses(
		(status = 200, body = ServerDetailData),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get_detail(
	State(state): State<AppState>,
	Json(args): Json<ServerIdArgs>,
) -> Result<Json<ServerDetailData>> {
	let db = state.db.clone();
	let mut conn = db.get().await?;
	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	let device_id = server.device_id;

	let (group, status, latest_version) = {
		let mut conn_group = db.get().await?;
		let mut conn_status = db.get().await?;

		let group_lookup = async {
			if let Some(gid) = server.group_id {
				ServerGroup::get_by_id(&mut conn_group, gid).await.map(Some)
			} else {
				Ok::<_, AppError>(None)
			}
		};

		let status_lookup = Status::latest_for_server(&mut conn_status, server.id);

		let latest_version_lookup = async {
			Version::get_latest_matching(&mut conn, "*".parse()?)
				.await
				.map(|v| v.as_semver())
		};

		let (group_result, status_result) = join(group_lookup, status_lookup).await;
		(group_result?, status_result?, latest_version_lookup.await?)
	};

	let mut server_details = server_to_info(server.clone());
	server_details.group_name = group.as_ref().map(|g| g.name.clone());

	let up = status
		.as_ref()
		.map(|s| s.short_status())
		.unwrap_or_default();
	let health = status
		.as_ref()
		.map(|s| s.health_state())
		.unwrap_or_default();

	let last_status = if let Some(st) = status.as_ref() {
		let device = if let Some(device_id) = st.device_id {
			DeviceConnection::get_latest_from_device_ids(&mut conn, [device_id].into_iter())
				.await?
				.into_iter()
				.next()
		} else {
			None
		};

		let platform = st.platform();
		let postgres = st.postgres_version();
		let nodejs = device.and_then(|d| d.nodejs_version());
		let version_distance = st.distance_from_version(&latest_version);
		let min_chrome_version = if let Some(ref version) = st.version {
			compute_min_chrome_version(&mut conn, version).await
		} else {
			None
		};

		Some(ServerLastStatusData {
			id: st.id,
			created_at: st.created_at,
			version: st.version.clone(),
			version_distance,
			min_chrome_version,
			platform,
			postgres,
			nodejs,
			timezone: st
				.extra("timezone")
				.and_then(|s| s.as_str().map(|s| s.to_string())),
			healthy: st.healthy,
			health: st.health.clone(),
			extra: st.extra.clone(),
		})
	} else {
		None
	};

	let device_info = if let Some(device_id) = device_id {
		let device_with_info = Device::get_with_info(&mut conn, device_id).await?;
		Some(super::devices::DeviceInfo::from_db(device_with_info, &state).await)
	} else {
		None
	};

	let siblings = if let Some(g) = group.as_ref() {
		let raw_siblings = server.siblings(&mut conn).await?;
		if raw_siblings.is_empty() {
			Vec::new()
		} else {
			let sibling_ids: Vec<Uuid> = raw_siblings.iter().map(|s| s.id).collect();
			let statuses = Status::latest_for_servers(&mut conn, &sibling_ids).await?;
			let status_map: std::collections::HashMap<Uuid, &Status> =
				statuses.iter().map(|s| (s.server_id, s)).collect();

			raw_siblings
				.into_iter()
				.map(|s| {
					let st = status_map.get(&s.id).copied();
					let sib_up = st.map(|s| s.short_status()).unwrap_or_default();
					let sib_health = st.map(|s| s.health_state()).unwrap_or_default();
					let mut info = server_to_info(s);
					info.group_name = Some(g.name.clone());
					(sib_up, sib_health, info)
				})
				.collect()
		}
	} else {
		Vec::new()
	};

	Ok(Json(ServerDetailData {
		server: server_details,
		device_info,
		last_status,
		up,
		health,
		group,
		siblings,
	}))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateArgs {
	pub server_id: Uuid,
	pub data: ServerDataUpdate,
}

#[utoipa::path(
	post,
	path = "/update",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = UpdateArgs,
	responses(
		(status = 200),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<UpdateArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;

	// `group_id` transitions go through Server::assign_to_group so that a
	// move from "ungrouped" → "grouped" can promote any pending issues into
	// an incident. Apply this before the rest of the field update so the
	// re-evaluation sees the same row state as the caller intends.
	let group_change = args.data.group_id;

	let update_data = PartialServer {
		id: args.server_id,
		name: args.data.name,
		kind: args.data.kind,
		rank: args.data.rank,
		host: if let Some(host_str) = args.data.host {
			Some(UrlField(host_str.parse().map_err(|e| {
				AppError::custom(format!("Invalid URL: {}", e))
			})?))
		} else {
			None
		},
		device_id: args.data.device_id,
		group_id: None,
		listed: args.data.listed,
		cloud: args.data.cloud,
		geolocation: args.data.geolocation,
		alert_when_down_for: args
			.data
			.alert_when_down_for
			.map(|s| database::pg_duration::PgDuration(jiff::SignedDuration::from_secs(s))),
		notes: args.data.notes,
		tags: args.data.tags,
	};
	Server::update(&mut conn, args.server_id, update_data).await?;

	if let Some(new_group_id) = group_change {
		Server::assign_to_group(&mut conn, args.server_id, new_group_id).await?;
	}
	Ok(Json(()))
}

#[derive(Deserialize, ToSchema)]
pub struct ImportTicketArgs {
	pub ticket_b64: String,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
}

#[utoipa::path(
	post,
	path = "/import_ticket",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = ImportTicketArgs,
	responses(
		(status = 200, description = "New server id.", body = Uuid, content_type = "application/json"),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn import_ticket(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ImportTicketArgs>,
) -> Result<Json<Uuid>> {
	let mut conn = state.db.get().await?;
	let ticket = CanopyTicket::from_base64(&args.ticket_b64).inspect_err(|e| {
		tracing::warn!(error = %e, "import_ticket: bad ticket payload");
	})?;
	let server = Server::upsert_from_ticket(&mut conn, &ticket, args.kind, args.rank)
		.await
		.inspect_err(|e| {
			tracing::warn!(error = %e, "import_ticket: upsert_from_ticket failed");
		})?;

	// If the ticket carries a Tailscale identity and we have a directory
	// configured, try to attach it to the device on a best-effort basis.
	// Failure here doesn't roll back the import — the operator can attach
	// manually via the device admin UI.
	if let Some(device_id) = server.device_id
		&& let Some(directory) = state.tailnet_directory.as_ref()
	{
		let identifier = ticket
			.tailscale_ip
			.as_deref()
			.or(ticket.tailscale_name.as_deref());
		if let Some(id) = identifier {
			match directory.resolve_identifier(id).await {
				Ok(Some(entry)) => {
					let identity = database::devices::TailscaleIdentity {
						node_id: entry.node_id.clone(),
						node_name: Some(entry.node_name),
						tailnet: Some(entry.tailnet),
					};
					match database::devices::Device::attach_tailscale(
						&mut conn, device_id, identity,
					)
					.await
					{
						Ok(()) => tracing::info!(
							%device_id,
							node_id = %entry.node_id,
							"import_ticket: auto-attached tailscale identity from ticket"
						),
						Err(e) => tracing::warn!(
							%device_id,
							error = %e,
							"import_ticket: could not auto-attach tailscale identity",
						),
					}
				}
				Ok(None) => tracing::info!(
					ticket_id = %id,
					"import_ticket: ticket's tailscale identifier not found in directory"
				),
				Err(e) => tracing::warn!(
					error = %e,
					"import_ticket: directory lookup failed"
				),
			}
		}
	}

	Ok(Json(server.id))
}

#[derive(Deserialize, ToSchema)]
pub struct AttachTailscaleDeviceArgs {
	pub server_id: Uuid,
	/// Any of: a Tailscale CGNAT/ULA IP, a node id, or a DNS name.
	pub identifier: String,
}

/// Find or create a `Device` row for a Tailscale node id resolved
/// from the supplied identifier, and attach it to the server
/// (`servers.device_id`). Used when a server has no device yet (e.g.
/// an operator-imported server that hasn't reported in) and the
/// operator wants to bind it to a tailnet node without going through
/// the device admin page first.
#[utoipa::path(
	post,
	path = "/attach_tailscale_device",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = AttachTailscaleDeviceArgs,
	responses(
		(status = 200, description = "Device id newly attached to the server.", body = Uuid, content_type = "application/json"),
		(status = 404, description = "Identifier does not resolve to a known tailnet node.", body = ProblemDetailsSchema),
		(status = 409, description = "The resolved device is already attached to another server.", body = ProblemDetailsSchema),
		(status = 503, description = "Tailnet directory not configured or unreachable.", body = ProblemDetailsSchema),
	),
)]
pub async fn attach_tailscale_device(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<AttachTailscaleDeviceArgs>,
) -> Result<Json<Uuid>> {
	let directory = state
		.tailnet_directory
		.as_ref()
		.ok_or(AppError::AuthTailnetDirectoryUnavailable)?;
	let entry = directory
		.resolve_identifier(&args.identifier)
		.await
		.map_err(|_| AppError::AuthTailnetDirectoryUnavailable)?
		.ok_or_else(|| AppError::BadRequest("no tailnet device matches that identifier".into()))?;

	let mut conn = state.db.get().await?;

	// Find existing device by node id, or create a new one.
	let device =
		if let Some(existing) = Device::from_tailscale_node_id(&mut conn, &entry.node_id).await? {
			existing
		} else {
			Device::create_with_tailscale(
				&mut conn,
				database::devices::TailscaleIdentity {
					node_id: entry.node_id.clone(),
					node_name: Some(entry.node_name.clone()),
					tailnet: Some(entry.tailnet.clone()),
				},
			)
			.await?
		};

	// Refuse if the device is already attached to a *different* server
	// — the operator should clear that one first.
	let other_servers = Server::get_by_device_id(&mut conn, device.id).await?;
	if other_servers.iter().any(|s| s.id != args.server_id) {
		return Err(AppError::Conflict(format!(
			"device {} is already attached to another server",
			device.id,
		)));
	}

	Server::update(
		&mut conn,
		args.server_id,
		PartialServer {
			id: args.server_id,
			name: None,
			kind: None,
			rank: None,
			host: None,
			device_id: Some(Some(device.id)),
			group_id: None,
			listed: None,
			cloud: None,
			geolocation: None,
			alert_when_down_for: None,
			notes: None,
			tags: None,
		},
	)
	.await?;

	Ok(Json(device.id))
}

pub(crate) async fn compute_min_chrome_version(
	conn: &mut database::diesel_async::AsyncPgConnection,
	version: &VersionStr,
) -> Option<u32> {
	let head_release_date = Version::get_head_release_date(conn, version.clone())
		.await
		.ok()?;
	database::chrome_releases::ChromeRelease::get_min_version_at_date(conn, head_release_date)
		.await
		.ok()?
}

pub(crate) async fn collect_group_names(
	conn: &mut database::diesel_async::AsyncPgConnection,
	servers: &[Server],
) -> Result<std::collections::HashMap<Uuid, String>> {
	let ids: Vec<Uuid> = servers.iter().filter_map(|s| s.group_id).collect();
	if ids.is_empty() {
		return Ok(std::collections::HashMap::new());
	}
	let groups = ServerGroup::list_by_ids(conn, &ids).await?;
	Ok(groups.into_iter().map(|g| (g.id, g.name)).collect())
}
