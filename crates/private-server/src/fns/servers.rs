use axum::Json;
use axum::extract::State;
use base64::Engine;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{
	Uuid,
	device::DeviceRole,
	geo::GeoPoint,
	server::{TagMap, kind::ServerKind, product::Product, rank::ServerRank},
	status::{HealthState, ShortStatus},
	version::VersionStr,
};
use database::{
	devices::{Device, DeviceConnection, TailscaleIdentity},
	pg_duration::PgDuration,
	reported_detail::ReportedDetail,
	server_enrollment_tokens::ServerEnrollmentToken,
	server_groups::ServerGroup,
	servers::{PartialServer, Server},
	statuses::Status,
	versions::Version,
};
use futures::future::join;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use utoipa::ToSchema;

use crate::fns::Page;
use crate::state::AppState;

/// Full detail view of a server: its own record, its bound device, its most
/// recent status report, current reachability/health, and group context.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerDetailData {
	/// The server's own record.
	pub server: ServerInfo,
	/// Full detail on the device bound to this server, if any.
	pub device_info: Option<super::devices::DeviceInfo>,
	/// The server's most recently reported status, if it has ever reported one.
	pub last_status: Option<ServerLastStatusData>,
	/// Current reachability, derived from the most recent status report.
	pub up: ShortStatus,
	/// Current self-reported health, derived from the most recent status report.
	pub health: HealthState,
	/// The server's current checks across every source, graded and
	/// classified — the live consolidated checks view.
	pub checks: commons_types::status::ConsolidatedChecks,
	/// The group this server belongs to, with its notes/tags so the UI can
	/// render the "Group" section without a second fetch. `None` when the
	/// server is ungrouped.
	pub group: Option<ServerGroup>,
	/// Other servers in the same group (excluding `server`). Empty when the
	/// server is ungrouped or alone in its group. Each entry carries its
	/// own `up` / `health` so the UI can render a status dot per sibling.
	pub siblings: Vec<ServerInfo>,
	/// The server's effective `billing.*` labels — i.e. its group's
	/// (product/deployment/stage). Empty when the server is ungrouped.
	pub billing_labels: Vec<super::server_groups::BillingTag>,
	/// Whether the server is known to run Munin, from the most recent source
	/// to report the flag. The UI offers a Munin link only when this is true.
	// spec: SVC#munin-link
	pub munin: bool,
}

/// A server in the fleet inventory: its identity, classification, network
/// address, monitoring configuration, and (when requested by the endpoint)
/// current reachability/health.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerInfo {
	/// Unique identifier for the server.
	pub id: Uuid,
	/// Operator-assigned name for the server, if any.
	pub name: Option<String>,
	/// The application this server runs. Decides which of canopy's
	/// per-server features apply to it.
	// spec: APP#product-and-kind
	pub product: Product,
	/// The server's role within its product's topology.
	pub kind: ServerKind,
	/// Where this server sits in its deployment's promotion order (e.g.
	/// production vs. staging), if applicable.
	pub rank: Option<ServerRank>,
	/// The server's stored URL, if any. May be absent for device-only servers.
	pub host: Option<String>,
	/// Effective URL for display: the stored `host`, or `https://{tailnet
	/// hostname}` when the server has no URL but is bound to a Tailscale
	/// device, else an empty string.
	pub display_host: String,
	/// The device currently bound to this server, if any.
	pub device_id: Option<Uuid>,
	/// The group this server belongs to, if any.
	pub group_id: Option<Uuid>,
	/// Display name of the group this server belongs to, included directly so
	/// list views don't need a separate lookup. `None` if ungrouped.
	pub group_name: Option<String>,
	/// Name this server appears under in the public mobile-app server list.
	/// `None` means the server is not listed publicly.
	pub public_name: Option<String>,
	/// Whether this server runs in a cloud environment, if known.
	pub cloud: Option<bool>,
	/// Geographic location of the server, if known.
	pub geolocation: Option<GeoPoint>,
	/// Whether canopy is actively watching this server. When `false`, the
	/// reachability sweep skips it and its issues don't contribute to
	/// incidents.
	pub is_monitored: bool,
	/// Threshold in seconds for the reachability sweep to consider this
	/// server down. Always positive; only consulted when `is_monitored`
	/// is `true`. The default at creation is 600 (10 minutes).
	pub alert_when_down_for: i64,
	/// Free-text operator notes about the server.
	pub notes: String,
	/// Arbitrary operator-defined key/value labels attached to the server.
	pub tags: TagMap,
	/// When a device completed enrollment for this server. `None` while
	/// awaiting first check-in, at which point setup instructions still apply.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub registered_at: Option<Timestamp>,
	/// Whether the server is archived (soft-deleted).
	pub archived: bool,
	/// Reachability of the server, derived from its most recent status
	/// report. Omitted when the endpoint that produced this response didn't
	/// batch-fetch statuses (e.g. the cheap list/get endpoints); a present
	/// value of "gone" means the lookup ran and found no status at all.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub up: Option<ShortStatus>,
	/// Self-reported health from the most recent status report. Same
	/// omitted-vs-present semantics as `up`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub health: Option<HealthState>,
}

/// The server's most recently reported status push: version/host info plus
/// health.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerLastStatusData {
	/// Unique identifier for this status report.
	pub id: Uuid,
	/// When this status was reported.
	pub created_at: Timestamp,
	/// Software version the server reported running, if known.
	pub version: Option<VersionStr>,
	/// How many releases behind the latest known version this server's
	/// reported version is, if it could be computed.
	pub version_distance: Option<u64>,
	/// Minimum Chrome version required to use this server's reported
	/// version, if determinable.
	pub min_chrome_version: Option<u32>,
	/// Operating system / platform the server reported, if any.
	pub platform: Option<String>,
	/// PostgreSQL version the server reported, if any.
	pub postgres: Option<String>,
	/// Node.js version associated with the server, if known.
	pub nodejs: Option<String>,
	/// Version of bestool, the agent reporting on the server. Absent when no
	/// source reports one.
	pub bestool: Option<String>,
	/// Timezone the server reported, if any.
	pub timezone: Option<String>,
	/// The source that pushed this status (e.g. `alertd`).
	pub source: String,
	/// Additional endpoint-defined data included with this status push.
	pub extra: JsonValue,
	/// Operators identified as connected to the server as of this push,
	/// with display details filled in where available. Whether this is
	/// still current is for the consumer to judge, alongside `up`.
	pub operators: Vec<commons_types::status::OperatorPresence>,
}

/// Partial update to a server's fields. Only fields present in the request
/// are changed. For `device_id`, `group_id`, `public_name`, `cloud`, and
/// `geolocation`, sending an explicit `null` clears the field, while
/// omitting it leaves the current value unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ServerDataUpdate {
	/// New name for the server. Omit to leave unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	/// New application for the server. Omit to leave unchanged. Changing it
	/// to a product that does not define the server's kind moves the kind to
	/// the new product's default, unless this request sets one too.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub product: Option<Product>,
	/// New role for the server within its product's topology. Omit to leave
	/// unchanged. Rejected when the product does not define it.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub kind: Option<ServerKind>,
	/// New promotion rank for the server. Omit to leave unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub rank: Option<ServerRank>,
	/// New URL for the server. An empty string clears it; omit to leave
	/// unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub host: Option<String>,
	/// New device to bind to the server, or `null` to unbind. Omit to leave
	/// unchanged.
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub device_id: Option<Option<Uuid>>,
	/// New group for the server, or `null` to remove it from its group.
	/// Omit to leave unchanged.
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub group_id: Option<Option<Uuid>>,
	/// New public-facing name for the server, or `null` to unlist it. Omit
	/// to leave unchanged.
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub public_name: Option<Option<String>>,
	/// Whether the server runs in a cloud environment, or `null` to clear.
	/// Omit to leave unchanged.
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub cloud: Option<Option<bool>>,
	/// New geographic location for the server, or `null` to clear it. Omit
	/// to leave unchanged.
	#[serde(
		default,
		deserialize_with = "deserialize_some",
		skip_serializing_if = "Option::is_none"
	)]
	pub geolocation: Option<Option<GeoPoint>>,
	/// Whether canopy should actively monitor this server. Omit to leave
	/// unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub is_monitored: Option<bool>,
	/// New downtime threshold in seconds before this server is considered
	/// down. Omit to leave unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub alert_when_down_for: Option<i64>,
	/// New free-text notes for the server. Omit to leave unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub notes: Option<String>,
	/// New set of operator-defined tags for the server. Omit to leave
	/// unchanged.
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
		product: s.product,
		kind: s.kind,
		rank: s.rank,
		host: s.host.as_ref().map(|h| h.0.to_string()),
		// Default the display host to the raw host; `fill_display_hosts` later
		// supplies the tailnet fallback for hostless, device-bound servers.
		display_host: s.host.as_ref().map(|h| h.0.to_string()).unwrap_or_default(),
		device_id: s.device_id,
		group_id: s.group_id,
		group_name: None,
		public_name: s.public_name,
		cloud: s.cloud,
		geolocation: s.geolocation,
		is_monitored: s.is_monitored,
		alert_when_down_for: s.alert_when_down_for.0.as_secs(),
		notes: s.notes,
		tags: s.tags,
		registered_at: s.registered_at,
		archived: s.deleted_at.is_some(),
		up: None,
		health: None,
	}
}

/// Batch-fetch the latest status for each server and write its short/health
/// representation onto the corresponding `ServerInfo`. Use this on listing
/// endpoints that feed a UI which renders status dots — skip it on cheap
/// fetches that don't surface the dot.
pub(super) async fn decorate_with_status(
	conn: &mut database::diesel_async::AsyncPgConnection,
	infos: &mut [ServerInfo],
) -> Result<()> {
	if infos.is_empty() {
		return Ok(());
	}
	let ids: Vec<Uuid> = infos.iter().map(|i| i.id).collect();
	let statuses = Status::latest_for_servers(conn, &ids).await?;
	let by_server: std::collections::HashMap<Uuid, &Status> =
		statuses.iter().map(|s| (s.server_id, s)).collect();
	// Health comes from current check state across every source
	// (silenced checks already skipped in the rollup).
	let server_groups: Vec<(Uuid, Option<Uuid>)> =
		infos.iter().map(|i| (i.id, i.group_id)).collect();
	let health = database::issues::health_from_check_state(conn, &server_groups).await?;
	for info in infos.iter_mut() {
		let st = by_server.get(&info.id).copied();
		info.up = Some(st.map(|s| s.short_status()).unwrap_or_default());
		info.health = Some(health.get(&info.id).copied().unwrap_or_default());
	}
	Ok(())
}

/// For servers with no stored URL but a bound device, set `display_host` to
/// `https://{tailnet hostname}`. (Servers with a URL already have
/// `display_host == host` from `server_to_info`.)
pub(super) async fn fill_display_hosts(
	conn: &mut database::diesel_async::AsyncPgConnection,
	infos: &mut [ServerInfo],
) -> Result<()> {
	let needy: Vec<Uuid> = infos
		.iter()
		.filter(|i| i.host.is_none())
		.filter_map(|i| i.device_id)
		.collect();
	if needy.is_empty() {
		return Ok(());
	}
	let names = Device::tailscale_names_by_ids(conn, &needy).await?;
	for info in infos.iter_mut() {
		if info.host.is_none()
			&& let Some(dev) = info.device_id
			&& let Some(name) = names.get(&dev)
		{
			info.display_host = format!("https://{name}");
		}
	}
	Ok(())
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
		.routes(routes!(list_archived))
		.routes(routes!(get_name))
		.routes(routes!(get_info))
		.routes(routes!(get_detail))
		.routes(routes!(update))
		.routes(routes!(create))
		.routes(routes!(delete))
		.routes(routes!(restore))
		.routes(routes!(mint_enrollment))
		.routes(routes!(revoke_enrollment))
		.routes(routes!(enrollment_status))
		.routes(routes!(attach_tailscale_device))
}

/// Filter and pagination parameters for listing servers.
#[derive(Deserialize, ToSchema)]
pub struct ServerListArgs {
	/// Restrict results to servers of this deployment kind. Omit to
	/// include all kinds.
	pub kind: Option<ServerKind>,
	/// Number of items to skip from the start of the result set.
	pub offset: u64,
	/// Maximum number of items to return.
	pub limit: Option<u64>,
}

/// List servers, optionally filtered by kind, paginated.
///
/// Returns a page of servers plus the total matching count. Entries include
/// their group name where applicable, but not current reachability/health —
/// use the detail endpoint for that.
#[utoipa::path(
	post,
	path = "/list_some",
	tag = "servers",
	request_body = ServerListArgs,
	responses(
		(status = 200, body = Page<ServerInfo>),
	),
)]
pub async fn list_some(
	State(state): State<AppState>,
	Json(args): Json<ServerListArgs>,
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
	let mut items: Vec<ServerInfo> = servers
		.into_iter()
		.map(|s| server_to_info_with_group(s, &group_names))
		.collect();
	fill_display_hosts(&mut conn, &mut items).await?;
	Ok(Json(Page { items, total }))
}

/// List servers that don't belong to any group.
///
/// Returns a page of ungrouped servers, each with current
/// reachability/health, plus the total count of ungrouped servers.
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
	let mut items: Vec<ServerInfo> = servers.into_iter().map(server_to_info).collect();
	decorate_with_status(&mut conn, &mut items).await?;
	fill_display_hosts(&mut conn, &mut items).await?;
	Ok(Json(Page { items, total }))
}

/// List archived (soft-deleted) servers.
///
/// Each entry has `archived: true` and includes current reachability/health.
/// Archived servers can be brought back with the restore endpoint.
#[utoipa::path(
	post,
	path = "/list_archived",
	tag = "servers",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, body = Vec<ServerInfo>),
	),
)]
pub async fn list_archived(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	_body: Json<serde_json::Value>,
) -> Result<Json<Vec<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let servers = Server::list_archived(&mut conn).await?;
	let mut items: Vec<ServerInfo> = servers.into_iter().map(server_to_info).collect();
	decorate_with_status(&mut conn, &mut items).await?;
	fill_display_hosts(&mut conn, &mut items).await?;
	Ok(Json(items))
}

/// Identifies a single server by id.
#[derive(Deserialize, ToSchema)]
pub struct ServerIdArgs {
	/// The server to operate on.
	pub server_id: Uuid,
}

/// Get a server's display name.
///
/// Returns the server's name if set, else its stored host, else its id —
/// always a non-empty string suitable for display. Returns 404 if no server
/// exists with that id.
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
		server
			.name
			.or_else(|| server.host.as_ref().map(|h| h.0.to_string()))
			.unwrap_or_else(|| server.id.to_string()),
	))
}

/// Get a server's basic record.
///
/// Returns identity, classification, and configuration for a single server,
/// including its group name where applicable. Does not include current
/// reachability/health or device/group detail — use the detail endpoint for
/// that. Returns 404 if no server exists with that id.
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
	fill_display_hosts(&mut conn, std::slice::from_mut(&mut info)).await?;
	Ok(Json(info))
}

/// Get full detail for a server.
///
/// Returns the server's record, its bound device (if any), its most recent
/// status report, current reachability/health, its group (if any) together
/// with sibling servers in the same group, and the group's billing labels.
/// Returns 404 if no server exists with that id.
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

		// A server detail page shouldn't 404 just because no versions are
		// published yet (e.g. a fresh deployment); treat "no match" as "unknown
		// latest" so `version_distance` falls back to None.
		let latest_version_lookup = async {
			match Version::get_latest_matching(&mut conn, "*".parse()?).await {
				Ok(v) => Ok::<_, AppError>(Some(v.as_semver())),
				Err(AppError::NoMatchingVersions) => Ok(None),
				Err(e) => Err(e),
			}
		};

		let (group_result, status_result) = join(group_lookup, status_lookup).await;
		(group_result?, status_result?, latest_version_lookup.await?)
	};

	let mut server_details = server_to_info(server.clone());
	server_details.group_name = group.as_ref().map(|g| g.name.clone());
	fill_display_hosts(&mut conn, std::slice::from_mut(&mut server_details)).await?;

	let up = status
		.as_ref()
		.map(|s| s.short_status())
		.unwrap_or_default();
	// One consolidated read drives both the headline health chip and the
	// checks table, so they can't disagree: the rollup and the list come
	// from the same graded state across every source.
	let checks =
		database::issues::consolidated_checks_latest(&mut conn, server.id, server.group_id).await?;
	let health = checks.health_state;

	let device_with_info = if let Some(device_id) = device_id {
		Some(Device::get_with_info(&mut conn, device_id).await?)
	} else {
		None
	};

	// Every source's current report on this server, resolved into one set of
	// figures. Independent of `status` above: that's the latest push and its
	// own metadata, this is what the server is running — which outlives any
	// one push, and survives the server going quiet.
	// spec: FIG#sourcing
	let figures = ReportedDetail::merge(&ReportedDetail::for_server(&mut conn, server.id).await?);

	let last_status = if let Some(st) = status.as_ref() {
		// The status usually comes from the server's own device, whose
		// latest connection was just fetched — only fall back to a
		// dedicated lookup when the status was pushed by a different one.
		let connection = match st.device_id {
			Some(did) if Some(did) == device_id => device_with_info
				.as_ref()
				.and_then(|d| d.latest_connection.clone()),
			Some(did) => DeviceConnection::get_latest_from_device_ids(&mut conn, [did].into_iter())
				.await?
				.into_iter()
				.next(),
			None => None,
		};

		// Prefer the payload-reported `nodeVersion`; fall back to the device
		// connection's User-Agent.
		let nodejs = figures
			.node_version()
			.or_else(|| connection.and_then(|d| d.nodejs_version()));
		let version_distance = latest_version
			.as_ref()
			.and_then(|lv| st.distance_from_version(lv));
		let min_chrome_version = if let Some(ref version) = st.version {
			compute_min_chrome_version(&mut conn, version).await
		} else {
			None
		};
		let mut operators = st.operators();
		super::statuses::enrich_operators(&mut conn, operators.iter_mut()).await?;

		Some(ServerLastStatusData {
			id: st.id,
			created_at: st.created_at,
			version: st.version.clone(),
			version_distance,
			min_chrome_version,
			platform: figures.platform(),
			postgres: figures.postgres_version(),
			nodejs,
			bestool: figures.bestool_version(),
			timezone: figures.timezone(),
			source: st.source.clone(),
			extra: st.extra.clone(),
			operators,
		})
	} else {
		None
	};

	let device_info = match device_with_info {
		Some(dwi) => Some(super::devices::DeviceInfo::from_db(dwi, &state).await),
		None => None,
	};

	let siblings = if let Some(g) = group.as_ref() {
		let raw_siblings = server.siblings(&mut conn).await?;
		let mut infos: Vec<ServerInfo> = raw_siblings
			.into_iter()
			.map(|s| {
				let mut info = server_to_info(s);
				info.group_name = Some(g.name.clone());
				info
			})
			.collect();
		decorate_with_status(&mut conn, &mut infos).await?;
		fill_display_hosts(&mut conn, &mut infos).await?;
		infos
	} else {
		Vec::new()
	};

	let billing_labels = match group.as_ref() {
		Some(g) => super::server_groups::group_billing_labels(&mut conn, g).await?,
		None => Vec::new(),
	};

	// spec: SVC#munin-link
	let munin = figures.munin().unwrap_or(false);

	Ok(Json(ServerDetailData {
		server: server_details,
		device_info,
		last_status,
		up,
		health,
		checks,
		group,
		siblings,
		billing_labels,
		munin,
	}))
}

/// Request to partially update a server.
#[derive(Deserialize, ToSchema)]
pub struct ServerUpdateArgs {
	/// The server to update.
	pub server_id: Uuid,
	/// The fields to change. Any field omitted is left unchanged.
	pub data: ServerDataUpdate,
}

/// Settle the product/kind pair an update leaves behind.
///
/// A product defines which roles exist, so the two can't move independently:
/// a kind the target product doesn't define is a bad request, and a product
/// change that leaves the stored kind undefined carries the server to the new
/// product's default role rather than stranding it on a role its product
/// doesn't have.
///
/// Reads the stored server only when the answer depends on it.
// spec: APP#product-and-kind
async fn settle_kind(
	conn: &mut database::diesel_async::AsyncPgConnection,
	server_id: Uuid,
	product: Option<Product>,
	kind: Option<ServerKind>,
) -> Result<Option<ServerKind>> {
	let target = match product {
		Some(product) => product,
		None => match kind {
			// Nothing to check: neither half of the pair is moving.
			None => return Ok(None),
			// The stored product has to define the requested kind.
			Some(_) => Server::get_by_id(conn, server_id).await?.product,
		},
	};

	match kind {
		Some(kind) if !target.defines_kind(kind) => Err(AppError::BadRequest(format!(
			"{target} servers have no {kind} role"
		))),
		Some(kind) => Ok(Some(kind)),
		// A product change with no kind: keep the stored kind when the new
		// product defines it, else move to that product's default.
		None => {
			let current = Server::get_by_id(conn, server_id).await?;
			Ok((!target.defines_kind(current.kind)).then(|| target.default_kind()))
		}
	}
}

/// Update a server's fields.
///
/// Applies a partial update — only the fields present in `data` are
/// changed. Moving a previously-ungrouped server into a group, or toggling
/// `is_monitored`, re-evaluates the server's open issues so incidents catch
/// up with the new state. Returns 400 if the update is rejected (e.g. an
/// invalid host value, or a role the target product doesn't define).
#[utoipa::path(
	post,
	path = "/update",
	operation_id = "server_update",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = ServerUpdateArgs,
	responses(
		(status = 200),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerUpdateArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;

	// Capture the server's pre-update state when this request touches either
	// of the two fields whose transitions warrant an incident catch-up:
	// `group_id` (ungrouped → grouped opens pending issues into incidents)
	// and `is_monitored` (un/monitored toggles incident eligibility
	// symmetrically — on enrols open issues, off cascades them out).
	let touches_catchup_field = args.data.group_id.is_some() || args.data.is_monitored.is_some();
	let before = if touches_catchup_field {
		Some(Server::get_by_id(&mut conn, args.server_id).await?)
	} else {
		None
	};
	let new_group_id = args.data.group_id;

	let kind = settle_kind(&mut conn, args.server_id, args.data.product, args.data.kind).await?;

	let update_data = PartialServer {
		id: args.server_id,
		name: args.data.name,
		product: args.data.product,
		kind,
		rank: args.data.rank,
		// `Some(Some(url))` sets, `Some(None)` clears, `None` leaves unchanged.
		// The form always sends `host`; an empty string clears it.
		host: match args.data.host {
			Some(s) if s.trim().is_empty() => Some(None),
			Some(s) => Some(Some(Server::canonicalize_host(&s)?)),
			None => None,
		},
		device_id: args.data.device_id,
		group_id: new_group_id,
		public_name: args.data.public_name,
		cloud: args.data.cloud,
		geolocation: args.data.geolocation,
		is_monitored: args.data.is_monitored,
		alert_when_down_for: args
			.data
			.alert_when_down_for
			.map(|s| database::pg_duration::PgDuration(jiff::SignedDuration::from_secs(s))),
		notes: args.data.notes,
		tags: args.data.tags,
	};
	Server::update(&mut conn, args.server_id, update_data).await?;

	let group_just_set = matches!(
		(before.as_ref().map(|s| s.group_id), new_group_id),
		(Some(None), Some(Some(_)))
	);
	let monitored_toggled = match (before.as_ref(), args.data.is_monitored) {
		(Some(b), Some(new_value)) => b.is_monitored != new_value,
		_ => false,
	};
	if group_just_set || monitored_toggled {
		database::issues::reevaluate_open_issues_for_server(&mut conn, args.server_id).await?;
	}
	Ok(Json(()))
}

/// Default downtime threshold for newly-created servers (10 minutes).
const DEFAULT_ALERT_SECS: i64 = 600;
/// Enrollment token lifetime: 7 days (human operational timescale).
const ENROLLMENT_TTL: jiff::SignedDuration = jiff::SignedDuration::from_hours(24 * 7);

/// Request to create a new server.
#[derive(Deserialize, ToSchema)]
pub struct CreateServerArgs {
	/// Name for the server, if any.
	pub name: Option<String>,
	/// URL for the server, if known. Can be added or changed later.
	#[serde(default)]
	pub host: Option<String>,
	/// The application this server runs. Defaults to tamanu.
	#[serde(default)]
	pub product: Product,
	/// The server's role within its product's topology. Rejected when the
	/// product does not define it.
	pub kind: ServerKind,
	/// Where this server sits in its deployment's promotion order (e.g.
	/// production vs. staging), if applicable.
	pub rank: Option<ServerRank>,
	/// Group to place the server in. Omit to create it ungrouped.
	pub group_id: Option<Uuid>,
	/// Name to list the server under in the public mobile-app server list.
	/// Omit to keep it unlisted.
	pub public_name: Option<String>,
	/// Whether the server runs in a cloud environment, if known.
	pub cloud: Option<bool>,
	/// Geographic location of the server, if known.
	pub geolocation: Option<GeoPoint>,
	/// Whether canopy should actively monitor this server. Defaults to
	/// true.
	pub is_monitored: Option<bool>,
	/// Downtime threshold in seconds before the server is considered
	/// down. Defaults to 600 (10 minutes).
	pub alert_when_down_for: Option<i64>,
	/// Free-text operator notes about the server.
	pub notes: Option<String>,
	/// Arbitrary operator-defined key/value labels for the server.
	pub tags: Option<TagMap>,
	/// Optional Tailscale identity to pre-bind a device to (an IP, node id,
	/// or DNS name). When given, a device is created for that identity
	/// immediately, and the key the machine presents when it later
	/// registers is added to that same device.
	pub tailscale_identifier: Option<String>,
}

/// Create a new server.
///
/// Creates the server record, optionally pre-bound to a Tailscale device
/// via `tailscale_identifier`, either ungrouped or in the given group.
#[utoipa::path(
	post,
	path = "/create",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = CreateServerArgs,
	responses(
		(status = 200, description = "New server id.", body = Uuid, content_type = "application/json"),
		(status = 400, body = ProblemDetailsSchema),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<CreateServerArgs>,
) -> Result<Json<Uuid>> {
	let mut conn = state.db.get().await?;

	let host = args
		.host
		.as_deref()
		.filter(|s| !s.trim().is_empty())
		.map(Server::canonicalize_host)
		.transpose()?;

	// Optionally pre-bind a Tailscale device.
	let device_id = if let Some(identifier) = args.tailscale_identifier.as_deref() {
		let directory = state
			.tailnet_directory
			.as_ref()
			.ok_or(AppError::AuthTailnetDirectoryUnavailable)?;
		let entry = directory
			.resolve_identifier(identifier)
			.await
			.map_err(|_| AppError::AuthTailnetDirectoryUnavailable)?
			.ok_or_else(|| {
				AppError::BadRequest("no tailnet device matches that identifier".into())
			})?;
		let device = match Device::from_tailscale_node_id(&mut conn, &entry.node_id).await? {
			Some(existing) => existing,
			None => {
				Device::create_with_tailscale(
					&mut conn,
					TailscaleIdentity {
						node_id: entry.node_id.clone(),
						node_name: Some(entry.node_name.clone()),
						tailnet: Some(entry.tailnet.clone()),
					},
					DeviceRole::Server,
				)
				.await?
			}
		};
		Some(device.id)
	} else {
		None
	};

	// A role its product doesn't define would leave the server misclassified
	// from the moment it exists.
	// spec: APP#product-and-kind
	if !args.product.defines_kind(args.kind) {
		return Err(AppError::BadRequest(format!(
			"{} servers have no {} role",
			args.product, args.kind
		)));
	}

	let server = Server {
		id: Uuid::new_v4(),
		name: args.name,
		host,
		product: args.product,
		kind: args.kind,
		rank: args.rank,
		device_id,
		group_id: args.group_id,
		public_name: args.public_name,
		cloud: args.cloud,
		geolocation: args.geolocation,
		is_monitored: args.is_monitored.unwrap_or(true),
		alert_when_down_for: PgDuration(jiff::SignedDuration::from_secs(
			args.alert_when_down_for.unwrap_or(DEFAULT_ALERT_SECS),
		)),
		notes: args.notes.unwrap_or_default(),
		tags: args.tags.unwrap_or_default(),
		deleted_at: None,
		registered_at: None,
		restore_allowed_until: None,
		restore_allowed_by: None,
	};

	let created = Server::create(&mut conn, server).await?;
	Ok(Json(created.id))
}

/// Identifies a single server by id.
#[derive(Deserialize, ToSchema)]
pub struct ServerIdOnlyArgs {
	/// The server to operate on.
	pub server_id: Uuid,
}

/// Archive (soft-delete) a server.
///
/// Releases and demotes its device. Archived servers no longer appear in
/// regular listings but can be restored later.
#[utoipa::path(
	post,
	path = "/delete",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = ServerIdOnlyArgs,
	responses(
		(status = 200),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn delete(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerIdOnlyArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Server::soft_delete(&mut conn, args.server_id).await?;
	Ok(Json(()))
}

/// Un-archive a server.
///
/// Restores a previously archived server to regular listings. Its machine
/// must re-enroll afterwards to rebind a device. Restoring a server that
/// isn't archived has no effect.
#[utoipa::path(
	post,
	path = "/restore",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = ServerIdOnlyArgs,
	responses(
		(status = 200),
		(status = 409, body = ProblemDetailsSchema),
	),
)]
pub async fn restore(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerIdOnlyArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Server::restore(&mut conn, args.server_id).await?;
	Ok(Json(()))
}

/// A freshly-minted enrollment ticket: the encrypted enrollment payload and
/// the passphrase that decrypts it.
#[derive(Serialize, ToSchema)]
pub struct EnrollmentTicket {
	/// Base64 (standard) of the age-encrypted enrollment JSON to feed to
	/// `bestool canopy register`. Encrypted under `passphrase` (age/scrypt), so
	/// it is safe to copy around on its own.
	pub ticket: String,
	/// Freshly-generated 4-word passphrase that decrypts `ticket`. Share this
	/// out-of-band (a separate channel from the ticket itself).
	pub passphrase: String,
	/// When the enrollment token inside the ticket expires.
	pub expires_at: Timestamp,
}

/// Mint (or reissue) an enrollment ticket for a server.
///
/// Creates a fresh enrollment token and returns it wrapped in a
/// passphrase-encrypted ticket the operator runs through bestool on the
/// enrolling machine, plus the 4-word passphrase that decrypts it. The
/// plaintext token lives only inside the encrypted ticket; reissuing
/// invalidates any prior token. Fails if the server is archived.
#[utoipa::path(
	post,
	path = "/mint_enrollment",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = ServerIdOnlyArgs,
	responses(
		(status = 200, body = EnrollmentTicket),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn mint_enrollment(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerIdOnlyArgs>,
) -> Result<Json<EnrollmentTicket>> {
	use algae_cli::{
		passphrases::{Passphrase, SecretString},
		streams::encrypt_stream,
	};

	let mut conn = state.db.get().await?;

	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	if server.deleted_at.is_some() {
		return Err(AppError::Conflict("server is archived".into()));
	}

	let api_url = std::env::var("PUBLIC_URL")
		.map_err(|_| AppError::custom("PUBLIC_URL is not configured"))?;

	let (token, plaintext) =
		ServerEnrollmentToken::mint(&mut conn, args.server_id, ENROLLMENT_TTL).await?;

	let payload = serde_json::json!({
		"v": "enroll-1",
		"api_url": api_url,
		"server_id": args.server_id,
		"token": plaintext,
	});
	let payload_bytes = serde_json::to_vec(&payload).map_err(AppError::custom)?;

	// Encrypt the payload with a fresh 4-word passphrase (age/scrypt), the same
	// primitives bestool's `protect`/`reveal` use. The ciphertext is base64'd
	// for transport; the passphrase travels out-of-band.
	let passphrase = crate::fns::generate_passphrase();
	let key = Passphrase::new(SecretString::from(passphrase.clone()));

	let mut encrypted = Vec::new();
	encrypt_stream(
		&payload_bytes[..],
		futures::io::Cursor::new(&mut encrypted),
		Box::new(key),
	)
	.await
	.map_err(|e| AppError::custom(format!("encrypting enrollment ticket: {e}")))?;

	let ticket = base64::engine::general_purpose::STANDARD.encode(&encrypted);

	Ok(Json(EnrollmentTicket {
		ticket,
		passphrase,
		expires_at: token.expires_at,
	}))
}

/// Revoke any outstanding enrollment ticket for a server.
///
/// Use this when a ticket was issued by mistake or is no longer needed.
/// Afterwards, the enrollment status endpoint reports no outstanding token,
/// and the revoked ticket can no longer be used to enroll.
#[utoipa::path(
	post,
	path = "/revoke_enrollment",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = ServerIdOnlyArgs,
	responses(
		(status = 200),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn revoke_enrollment(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerIdOnlyArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	ServerEnrollmentToken::revoke(&mut conn, args.server_id).await?;
	Ok(Json(()))
}

/// A server's enrollment state: whether a device has registered, and
/// whether an enrollment token is currently outstanding.
#[derive(Serialize, ToSchema)]
pub struct EnrollmentStatus {
	/// When enrollment completed. Omitted while still awaiting the first
	/// check-in.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub registered_at: Option<Timestamp>,
	/// Expiry of the currently-active enrollment token, if one is outstanding.
	/// Never reveals the token itself.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub token_expires_at: Option<Timestamp>,
	/// When the currently-active enrollment token was issued, if one is
	/// outstanding — e.g. to show "a ticket was issued on <date>".
	#[serde(skip_serializing_if = "Option::is_none")]
	pub token_issued_at: Option<Timestamp>,
}

/// Get the enrollment state of a server.
///
/// Reports whether the server has completed enrollment, and whether an
/// enrollment token is currently outstanding (issue and expiry times only —
/// the token itself is never revealed).
#[utoipa::path(
	post,
	path = "/enrollment_status",
	tag = "servers",
	security(("tailscale-admin" = [])),
	request_body = ServerIdOnlyArgs,
	responses(
		(status = 200, body = EnrollmentStatus),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn enrollment_status(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ServerIdOnlyArgs>,
) -> Result<Json<EnrollmentStatus>> {
	let mut conn = state.db.get().await?;
	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	let active = ServerEnrollmentToken::active_for(&mut conn, args.server_id).await?;
	Ok(Json(EnrollmentStatus {
		registered_at: server.registered_at,
		token_expires_at: active.as_ref().map(|t| t.expires_at),
		token_issued_at: active.as_ref().map(|t| t.created_at),
	}))
}

/// Request to bind a server to a device identified by its Tailscale node.
#[derive(Deserialize, ToSchema)]
pub struct AttachTailscaleDeviceArgs {
	/// The server to attach the device to.
	pub server_id: Uuid,
	/// Any of: a Tailscale CGNAT/ULA IP, a node id, or a DNS name.
	pub identifier: String,
}

/// Attach a device to a server via a Tailscale identifier.
///
/// Resolves the identifier to a tailnet node, finds the device already
/// attached to that node or creates a new one for it, and binds that device
/// to the server. Useful when a server has no device yet (e.g. an
/// operator-imported server that hasn't reported in) and should be bound to
/// a tailnet node directly. Returns 409 if the resolved device is already
/// attached to another live server — detach it there first.
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
				DeviceRole::Server,
			)
			.await?
		};

	// Refuse if the device is already attached to a *different* live server
	// — the operator should clear that one first. Archived servers don't count
	// (their device is already released), so scope to the live set.
	let other_servers = Server::live_by_device_id(&mut conn, device.id).await?;
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
			product: None,
			kind: None,
			rank: None,
			host: None,
			device_id: Some(Some(device.id)),
			group_id: None,
			public_name: None,
			cloud: None,
			geolocation: None,
			is_monitored: None,
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
