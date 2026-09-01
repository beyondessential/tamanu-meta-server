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
	server::{TagMap, app_type::ApplicationType, rank::ServerRank},
	status::{HealthState, ShortStatus},
	version::VersionStr,
};
use database::{
	applications::{Application, PartialServer},
	devices::{Device, DeviceConnection},
	reported_detail::ReportedDetail,
	server_enrollment_tokens::ServerEnrollmentToken,
	server_groups::ServerGroup,
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
	/// Whether a maintenance window suspends this server, its own or its
	/// group's: its checks are recorded and shown, and raise nothing.
	// spec: MNT#presentation
	pub maintained: bool,
	/// Whether the suspension is only the settle period: the window has
	/// ended and watching resumes when it elapses.
	pub maintenance_settling: bool,
	/// The server's current checks across every source, graded and
	/// classified — the live consolidated checks view.
	pub checks: commons_types::status::ConsolidatedChecks,
	/// The group this server belongs to, with its notes/tags so the UI can
	/// render the "Group" section without a second fetch. `None` when the
	/// server is ungrouped.
	pub group: Option<ServerGroup>,
	/// Other applications in the same group (excluding `server`). Empty when the
	/// server is ungrouped or alone in its group. Each entry carries its
	/// own `up` / `health` so the UI can render a status dot per sibling.
	pub siblings: Vec<ServerInfo>,
	/// The server's own effective `billing.*` labels
	/// (product/deployment/stage) — the ones canopy hands the server's device,
	/// carrying its own product and rank rather than its group's. Empty when
	/// the server is ungrouped, there being no deployment to attribute to.
	// spec: APP#billing-attribution
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
	/// The machine this application runs on. Maintenance is declared over the
	/// machine, so a surface offering to declare one needs it (see MNT).
	pub machine_id: Uuid,
	/// Operator-assigned name for the server, if any.
	pub name: Option<String>,
	/// What this application is: the software and the role it plays together.
	/// Decides which of Canopy's per-application features apply to it.
	// spec: APP
	pub r#type: ApplicationType,
	/// Where this server sits in its deployment's promotion order (e.g.
	/// production vs. staging), if applicable.
	pub rank: Option<ServerRank>,
	/// The server's stored URL, if any. May be absent for device-only applications.
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
	/// Whether the server may manage its own DNS records for names under its
	/// group's domains.
	pub may_manage_dns: bool,
	/// Whether the server may obtain TLS certificates for names under its
	/// group's domains.
	pub may_manage_tls: bool,
	/// Whether a maintenance window suspends this server, its own or its
	/// group's. Set alongside `up` and `health` by the endpoints that
	/// decorate listings; `None` where they aren't.
	// spec: MNT#presentation
	pub maintained: Option<bool>,
}

/// The server's most recently reported status push: version/host info plus
/// health.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerLastStatusData {
	/// Unique identifier for this status report.
	pub id: Uuid,
	/// When this status was reported.
	pub created_at: Timestamp,
	/// What the application is. Travels with the version so a consumer can
	/// tell a type with no version from one that has yet to report one.
	// spec: APP#capabilities
	pub r#type: ApplicationType,
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
	/// Whether this server may manage its own DNS records for names under its
	/// group's domains. Omit to leave unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub may_manage_dns: Option<bool>,
	/// Whether this server may obtain TLS certificates for names under its
	/// group's domains. Omit to leave unchanged.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub may_manage_tls: Option<bool>,
}

fn deserialize_some<'de, T, D>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
	T: Deserialize<'de>,
	D: serde::Deserializer<'de>,
{
	Deserialize::deserialize(deserializer).map(Some)
}

pub(super) fn server_to_info(s: Application) -> ServerInfo {
	ServerInfo {
		id: s.id,
		machine_id: s.machine_id,
		name: s.name,
		r#type: s.r#type,
		rank: s.rank,
		host: s.host.as_ref().map(|h| h.0.to_string()),
		// Default the display host to the raw host; `fill_display_hosts` later
		// supplies the tailnet fallback for hostless, device-bound applications.
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
		maintained: None,
		may_manage_dns: s.may_manage_dns,
		may_manage_tls: s.may_manage_tls,
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
	let (maintained_servers, maintained_groups) =
		database::maintenance_windows::MaintenanceWindow::suspended_targets(conn).await?;
	for info in infos.iter_mut() {
		let st = by_server.get(&info.id).copied();
		// Each application's own threshold, carried on the info we are filling.
		// spec: CHK#reachability
		let down_after = jiff::SignedDuration::from_secs(info.alert_when_down_for);
		info.up = Some(st.map(|s| s.short_status(down_after)).unwrap_or_default());
		info.health = Some(health.get(&info.id).copied().unwrap_or_default());
		info.maintained = Some(
			maintained_servers.contains(&info.id)
				|| info
					.group_id
					.is_some_and(|gid| maintained_groups.contains(&gid)),
		);
	}
	Ok(())
}

/// For applications with no stored URL but a bound device, set `display_host` to
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
	s: Application,
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
		.routes(routes!(delete))
		.routes(routes!(restore))
		.routes(routes!(mint_enrollment))
		.routes(routes!(revoke_enrollment))
		.routes(routes!(enrollment_status))
		.routes(routes!(attach_tailscale_device))
}

/// Filter and pagination parameters for listing applications.
#[derive(Deserialize, ToSchema)]
pub struct ServerListArgs {
	/// Restrict results to applications of this type. Omit to include all.
	pub r#type: Option<ApplicationType>,
	/// Number of items to skip from the start of the result set.
	pub offset: u64,
	/// Maximum number of items to return.
	pub limit: Option<u64>,
}

/// List applications, optionally filtered by kind, paginated.
///
/// Returns a page of applications plus the total matching count. Entries include
/// their group name where applicable, but not current reachability/health —
/// use the detail endpoint for that.
#[utoipa::path(
	post,
	path = "/list_some",
	tag = "applications",
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
	let total = if let Some(r#type) = args.r#type {
		Application::count_by_type(&mut conn, r#type).await?
	} else {
		Application::count_all(&mut conn).await?
	};
	let applications = if let Some(r#type) = args.r#type {
		Application::list_by_type(&mut conn, r#type, args.offset, args.limit).await?
	} else {
		Application::get_all(&mut conn, args.offset, args.limit).await?
	};
	let group_names = collect_group_names(&mut conn, &applications).await?;
	let mut items: Vec<ServerInfo> = applications
		.into_iter()
		.map(|s| server_to_info_with_group(s, &group_names))
		.collect();
	fill_display_hosts(&mut conn, &mut items).await?;
	Ok(Json(Page { items, total }))
}

/// List applications that don't belong to any group.
///
/// Returns a page of ungrouped applications, each with current
/// reachability/health, plus the total count of ungrouped applications.
#[utoipa::path(
	post,
	path = "/list_ungrouped",
	tag = "applications",
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
	let total = Application::count_ungrouped(&mut conn).await?;
	let applications = Application::list_ungrouped(&mut conn).await?;
	let mut items: Vec<ServerInfo> = applications.into_iter().map(server_to_info).collect();
	decorate_with_status(&mut conn, &mut items).await?;
	fill_display_hosts(&mut conn, &mut items).await?;
	Ok(Json(Page { items, total }))
}

/// List archived (soft-deleted) applications.
///
/// Each entry has `archived: true` and includes current reachability/health.
/// Archived applications can be brought back with the restore endpoint.
#[utoipa::path(
	post,
	path = "/list_archived",
	tag = "applications",
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
	let applications = Application::list_archived(&mut conn).await?;
	let mut items: Vec<ServerInfo> = applications.into_iter().map(server_to_info).collect();
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
	tag = "applications",
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
	let server = Application::get_by_id(&mut conn, args.server_id).await?;
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
	tag = "applications",
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
	let server = Application::get_by_id(&mut conn, args.server_id).await?;
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
/// with sibling applications in the same group, and the group's billing labels.
/// Returns 404 if no server exists with that id.
#[utoipa::path(
	post,
	path = "/get_detail",
	tag = "applications",
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
	let server = Application::get_by_id(&mut conn, args.server_id).await?;
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

	let up = server.reachability(status.as_ref());
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
		// Grading a version means measuring it against a release train canopy
		// holds, so both the distance and the embedded-browser floor apply only
		// to a product that has one. A canopy instance reports its own build
		// version and would otherwise be measured against Tamanu's releases.
		// spec: APP#versions
		let graded = server.r#type.tracks_versions();
		let version_distance = latest_version
			.as_ref()
			.filter(|_| graded)
			.and_then(|lv| st.distance_from_version(lv));
		let min_chrome_version = match &st.version {
			Some(version) if graded => compute_min_chrome_version(&mut conn, version).await,
			_ => None,
		};
		let mut operators = st.operators();
		super::statuses::enrich_operators(&mut conn, operators.iter_mut()).await?;

		Some(ServerLastStatusData {
			id: st.id,
			created_at: st.created_at,
			r#type: server.r#type,
			// A product with no application version presents none, as against
			// the `unknown` a versioned server shows before it has reported.
			// spec: APP#versions
			version: st.version.clone().filter(|_| server.r#type.has_versions()),
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

	// This server's own attribution, not its group's: for a server whose
	// product or rank differs from the group's, the page would otherwise show
	// labels the device is never handed.
	// spec: APP#billing-attribution
	let billing_labels = match group.as_ref() {
		Some(g) => commons_servers::backup_jobs::BillingLabels::for_server(
			&g.tags,
			&g.name,
			server.r#type,
			server.rank,
		)
		.into_tags()
		.into_iter()
		.map(|(key, value)| super::server_groups::BillingTag { key, value })
		.collect(),
		None => Vec::new(),
	};

	// spec: SVC#munin-link
	let munin = figures.munin().unwrap_or(false);

	// An application is under maintenance when the box it runs on is: work on
	// the machine stops the workload whether or not anyone named it.
	let maintained = database::maintenance_windows::MaintenanceWindow::suspends(
		&mut conn,
		Some(server.machine_id),
		server.group_id,
	)
	.await?;
	let maintenance_settling = maintained && {
		use database::issues::Scope;
		use database::maintenance_windows::MaintenanceWindow;
		let mut open = MaintenanceWindow::open_for(&mut conn, Scope::Machine(server.machine_id))
			.await?
			.is_some();
		if !open && let Some(gid) = server.group_id {
			open = MaintenanceWindow::open_for(&mut conn, Scope::Group(gid))
				.await?
				.is_some();
		}
		!open
	};

	Ok(Json(ServerDetailData {
		server: server_details,
		device_info,
		last_status,
		up,
		health,
		maintained,
		maintenance_settling,
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
	tag = "applications",
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
		Some(Application::get_by_id(&mut conn, args.server_id).await?)
	} else {
		None
	};
	// An application's group is never set independently of its machine's, so a
	// group change here is applied to the box. That keeps this endpoint's
	// contract while giving it the model's meaning: moving "the server" to a
	// group moves the machine, and the applications on it follow.
	//
	// Routed through `Machine::update` rather than a column write so the
	// consequences come with it — open issues re-evaluated for anything that
	// gains a group, and both groups' cached effective version recomputed.
	// spec: FLT#groups
	let new_group_id = args.data.group_id;
	if let Some(group_id) = new_group_id {
		let application = Application::get_by_id(&mut conn, args.server_id).await?;
		database::machines::Machine::update(
			&mut conn,
			application.machine_id,
			database::machines::MachineUpdate {
				group_id: Some(group_id),
				..Default::default()
			},
		)
		.await?;
	}

	let update_data = PartialServer {
		id: args.server_id,
		name: args.data.name,
		rank: args.data.rank,
		// `Some(Some(url))` sets, `Some(None)` clears, `None` leaves unchanged.
		// The form always sends `host`; an empty string clears it.
		host: match args.data.host {
			Some(s) if s.trim().is_empty() => Some(None),
			Some(s) => Some(Some(Application::canonicalize_host(&s)?)),
			None => None,
		},
		device_id: args.data.device_id,
		// Deliberately absent: the group came from the machine above.
		group_id: None,
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
		may_manage_dns: args.data.may_manage_dns,
		may_manage_tls: args.data.may_manage_tls,
	};
	Application::update(&mut conn, args.server_id, update_data).await?;

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

/// Default downtime threshold for newly-created applications (10 minutes).
const DEFAULT_ALERT_SECS: i64 = 600;
/// Enrollment token lifetime: 7 days (human operational timescale).
const ENROLLMENT_TTL: jiff::SignedDuration = jiff::SignedDuration::from_hours(24 * 7);

/// Identifies a single server by id.
#[derive(Deserialize, ToSchema)]
pub struct ServerIdOnlyArgs {
	/// The server to operate on.
	pub server_id: Uuid,
}

/// Archive (soft-delete) a server.
///
/// Releases and demotes its device. Archived applications no longer appear in
/// regular listings but can be restored later.
#[utoipa::path(
	post,
	path = "/delete",
	tag = "applications",
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
	Application::soft_delete(&mut conn, args.server_id).await?;
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
	tag = "applications",
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
	Application::restore(&mut conn, args.server_id).await?;
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
	tag = "applications",
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

	let server = Application::get_by_id(&mut conn, args.server_id).await?;
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
	tag = "applications",
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
	tag = "applications",
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
	let server = Application::get_by_id(&mut conn, args.server_id).await?;
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
	tag = "applications",
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
		.ok_or_else(|| AppError::NotFound("no tailnet device matches that identifier".into()))?;

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
	// — the operator should clear that one first. Archived applications don't count
	// (their device is already released), so scope to the live set.
	let other_servers = Application::live_by_device_id(&mut conn, device.id).await?;
	if other_servers.iter().any(|s| s.id != args.server_id) {
		return Err(AppError::Conflict(format!(
			"device {} is already attached to another server",
			device.id,
		)));
	}

	Application::update(
		&mut conn,
		args.server_id,
		PartialServer {
			id: args.server_id,
			name: None,
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
			may_manage_dns: None,
			may_manage_tls: None,
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
	applications: &[Application],
) -> Result<std::collections::HashMap<Uuid, String>> {
	let ids: Vec<Uuid> = applications.iter().filter_map(|s| s.group_id).collect();
	if ids.is_empty() {
		return Ok(std::collections::HashMap::new());
	}
	let groups = ServerGroup::list_by_ids(conn, &ids).await?;
	Ok(groups.into_iter().map(|g| (g.id, g.name)).collect())
}
