use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailnet_directory::DirectoryEntry;
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{Uuid, device::DeviceRole};
use database::devices::{Device, DeviceConnection, DeviceKey, DeviceWithInfo, TailscaleIdentity};
use database::servers::Server;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::fns::Page;
use crate::fns::servers::ServerInfo;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceInfo {
	pub device: DeviceData,
	pub keys: Vec<DeviceKeyInfo>,
	pub latest_connection: Option<DeviceConnectionData>,
	/// Live snapshot from the Tailscale control plane for a device
	/// that's currently attached by `tailscale_node_id`. `None` if the
	/// device has no tailnet attachment, the directory isn't
	/// configured, or the node id isn't in the directory's cache.
	#[serde(skip_serializing_if = "Option::is_none", default)]
	pub tailnet_live: Option<TailnetLiveInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TailnetLiveInfo {
	pub node_id: String,
	pub display_name: String,
	pub tailnet: String,
	pub addresses: Vec<String>,
	pub tags: Vec<String>,
}

impl From<DirectoryEntry> for TailnetLiveInfo {
	fn from(e: DirectoryEntry) -> Self {
		Self {
			node_id: e.node_id,
			display_name: e.node_name,
			tailnet: e.tailnet,
			addresses: e.addresses.into_iter().map(|a| a.to_string()).collect(),
			tags: e.tags,
		}
	}
}

impl DeviceInfo {
	pub fn name(&self) -> String {
		self.keys
			.iter()
			.filter_map(|key| {
				key.name
					.as_ref()
					.filter(|name| *name != "Initial Key")
					.cloned()
			})
			.next_back()
			.or_else(|| {
				self.latest_connection
					.as_ref()
					.map(|conn| conn.ip.to_string())
			})
			.unwrap_or_else(|| self.device.id.to_string())
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceData {
	pub id: Uuid,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
	pub role: DeviceRole,
	/// The Tailscale node ID this device is attached to, if any. The
	/// live IP / display name corresponding to this id is in
	/// [`DeviceInfo::tailnet_live`].
	#[serde(skip_serializing_if = "Option::is_none", default)]
	pub tailscale_node_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none", default)]
	pub tailscale_node_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none", default)]
	pub tailscale_tailnet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceKeyInfo {
	pub id: Uuid,
	pub device_id: Uuid,
	pub name: Option<String>,
	pub pem_data: String,
	pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceConnectionData {
	pub id: Uuid,
	pub created_at: Timestamp,
	pub device_id: Uuid,
	pub ip: String,
	pub user_agent: Option<String>,
}

impl From<DeviceWithInfo> for DeviceInfo {
	fn from(d: DeviceWithInfo) -> Self {
		Self {
			device: DeviceData {
				id: d.device.id,
				created_at: d.device.created_at,
				updated_at: d.device.updated_at,
				role: d.device.role,
				tailscale_node_id: d.device.tailscale_node_id,
				tailscale_node_name: d.device.tailscale_node_name,
				tailscale_tailnet: d.device.tailscale_tailnet,
			},
			keys: d.keys.into_iter().map(DeviceKeyInfo::from).collect(),
			latest_connection: d.latest_connection.map(DeviceConnectionData::from),
			tailnet_live: None,
		}
	}
}

impl DeviceInfo {
	/// Populate `tailnet_live` from the directory if this device has a
	/// `tailscale_node_id` and the directory has it cached.
	async fn enrich_with_live(mut self, state: &AppState) -> Self {
		if let Some(node_id) = self.device.tailscale_node_id.clone()
			&& let Some(directory) = &state.tailnet_directory
			&& let Ok(Some(entry)) = directory.find_by_node_id(&node_id).await
		{
			self.tailnet_live = Some(entry.into());
		}
		self
	}
}

impl From<DeviceKey> for DeviceKeyInfo {
	fn from(key: DeviceKey) -> Self {
		Self {
			id: key.id,
			device_id: key.device_id,
			name: key.name,
			pem_data: format_key_as_pem(&key.key_data),
			created_at: key.created_at,
		}
	}
}

impl From<DeviceConnection> for DeviceConnectionData {
	fn from(conn: DeviceConnection) -> Self {
		Self {
			id: conn.id,
			created_at: conn.created_at,
			device_id: conn.device_id,
			ip: conn.ip.addr().to_string(),
			user_agent: conn.user_agent,
		}
	}
}

fn format_key_as_pem(key_data: &[u8]) -> String {
	use base64::prelude::*;
	let base64_data = BASE64_STANDARD.encode(key_data);
	let mut pem = String::with_capacity(base64_data.len() + 100);
	pem.push_str("-----BEGIN PUBLIC KEY-----\n");
	for chunk in base64_data.as_bytes().chunks(64) {
		pem.push_str(&String::from_utf8_lossy(chunk));
		pem.push('\n');
	}
	pem.push_str("-----END PUBLIC KEY-----");
	pem
}

fn server_to_info(s: database::servers::Server) -> ServerInfo {
	ServerInfo {
		id: s.id,
		name: s.name,
		host: s.host.into(),
		kind: s.kind,
		rank: s.rank,
		device_id: s.device_id,
		parent_server_id: s.parent_server_id,
		parent_server_name: None,
		listed: s.listed,
		cloud: s.cloud,
		geolocation: s.geolocation,
		alert_when_down: s.alert_when_down,
	}
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(get_device_by_id))
		.routes(routes!(list_untrusted))
		.routes(routes!(get_servers_for_device))
		.routes(routes!(get_past_server_associations))
		.routes(routes!(connection_history))
		.routes(routes!(connection_count))
		.routes(routes!(trust))
		.routes(routes!(list_trusted))
		.routes(routes!(untrust))
		.routes(routes!(update_role))
		.routes(routes!(search))
		.routes(routes!(update_key_name))
		.routes(routes!(attach_tailscale))
		.routes(routes!(detach_tailscale))
		.routes(routes!(merge_into))
		.routes(routes!(resolve_tailnet_identifier))
}

#[derive(Deserialize, ToSchema)]
pub struct DeviceIdArgs {
	pub device_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/get_device_by_id",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = DeviceIdArgs,
	responses(
		(status = 200, body = DeviceInfo),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get_device_by_id(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<DeviceInfo>> {
	let mut conn = state.db.get().await?;
	let device_with_info = Device::get_with_info(&mut conn, args.device_id).await?;
	let info = DeviceInfo::from(device_with_info).enrich_with_live(&state).await;
	Ok(Json(info))
}

#[derive(Deserialize, ToSchema)]
pub struct PaginationArgs {
	pub offset: u64,
	pub limit: Option<u64>,
}

#[utoipa::path(
	post,
	path = "/list_untrusted",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = PaginationArgs,
	responses(
		(status = 200, body = Page<DeviceInfo>),
	),
)]
pub async fn list_untrusted(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<PaginationArgs>,
) -> Result<Json<Page<DeviceInfo>>> {
	let mut conn = state.db.get().await?;
	let total = Device::count_untrusted(&mut conn)
		.await?
		.try_into()
		.unwrap_or(0);
	let devices_with_info = Device::list_untrusted_with_info_paginated(
		&mut conn,
		args.limit.unwrap_or(10).try_into().unwrap_or(10),
		args.offset.try_into().unwrap_or(0),
	)
	.await?;
	let items = devices_with_info
		.into_iter()
		.map(DeviceInfo::from)
		.collect();
	Ok(Json(Page { items, total }))
}

#[utoipa::path(
	post,
	path = "/get_servers_for_device",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = DeviceIdArgs,
	responses(
		(status = 200, body = Vec<ServerInfo>),
	),
)]
pub async fn get_servers_for_device(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<Vec<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let servers = Server::get_by_device_id(&mut conn, args.device_id).await?;
	Ok(Json(servers.into_iter().map(server_to_info).collect()))
}

#[utoipa::path(
	post,
	path = "/get_past_server_associations",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = DeviceIdArgs,
	responses(
		(status = 200, body = Vec<ServerInfo>),
	),
)]
pub async fn get_past_server_associations(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<Vec<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let servers = Server::get_past_associations_for_device(&mut conn, args.device_id).await?;
	Ok(Json(servers.into_iter().map(server_to_info).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct HistoryCursor {
	pub created_at: Timestamp,
	pub id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct ConnectionHistoryArgs {
	pub device_id: Uuid,
	pub before: Option<HistoryCursor>,
	pub limit: Option<u64>,
}

#[utoipa::path(
	post,
	path = "/connection_history",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = ConnectionHistoryArgs,
	responses(
		(status = 200, body = Vec<DeviceConnectionData>),
	),
)]
pub async fn connection_history(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ConnectionHistoryArgs>,
) -> Result<Json<Vec<DeviceConnectionData>>> {
	let mut conn = state.db.get().await?;
	let before = args.before.map(|c| (c.created_at, c.id));
	let connections = DeviceConnection::get_history_for_device(
		&mut conn,
		args.device_id,
		before,
		args.limit.unwrap_or(100).try_into().unwrap_or(100),
	)
	.await?;
	Ok(Json(
		connections
			.into_iter()
			.map(DeviceConnectionData::from)
			.collect(),
	))
}

#[utoipa::path(
	post,
	path = "/connection_count",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = DeviceIdArgs,
	responses(
		(status = 200, body = u64, content_type = "application/json"),
	),
)]
pub async fn connection_count(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<u64>> {
	let mut conn = state.db.get().await?;
	Ok(Json(
		DeviceConnection::get_connection_count_for_device(&mut conn, args.device_id)
			.await?
			.try_into()
			.unwrap_or_default(),
	))
}

#[derive(Deserialize, ToSchema)]
pub struct TrustArgs {
	pub device_id: Uuid,
	pub role: DeviceRole,
}

#[utoipa::path(
	post,
	path = "/trust",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = TrustArgs,
	responses(
		(status = 200),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn trust(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<TrustArgs>,
) -> Result<Json<()>> {
	if args.role == DeviceRole::Untrusted {
		return Err(AppError::custom("Cannot set device role to untrusted"));
	}
	let mut conn = state.db.get().await?;
	Device::trust(&mut conn, args.device_id, args.role).await?;
	Ok(Json(()))
}

#[utoipa::path(
	post,
	path = "/list_trusted",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = PaginationArgs,
	responses(
		(status = 200, body = Page<DeviceInfo>),
	),
)]
pub async fn list_trusted(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<PaginationArgs>,
) -> Result<Json<Page<DeviceInfo>>> {
	let mut conn = state.db.get().await?;
	let total = Device::count_trusted(&mut conn)
		.await?
		.try_into()
		.unwrap_or(0);
	let devices_with_info = Device::list_trusted_with_info_paginated(
		&mut conn,
		args.limit.unwrap_or(10).try_into().unwrap_or(10),
		args.offset.try_into().unwrap_or(0),
	)
	.await?;
	let items = devices_with_info
		.into_iter()
		.map(DeviceInfo::from)
		.collect();
	Ok(Json(Page { items, total }))
}

#[utoipa::path(
	post,
	path = "/untrust",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = DeviceIdArgs,
	responses(
		(status = 200),
	),
)]
pub async fn untrust(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Device::untrust(&mut conn, args.device_id).await?;
	Ok(Json(()))
}

#[utoipa::path(
	post,
	path = "/update_role",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = TrustArgs,
	responses(
		(status = 200),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn update_role(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<TrustArgs>,
) -> Result<Json<()>> {
	if args.role == DeviceRole::Untrusted {
		return Err(AppError::custom(
			"Use untrust function to set device role to untrusted",
		));
	}
	let mut conn = state.db.get().await?;
	Device::trust(&mut conn, args.device_id, args.role).await?;
	Ok(Json(()))
}

#[derive(Deserialize, ToSchema)]
pub struct SearchArgs {
	pub query: String,
}

#[utoipa::path(
	post,
	path = "/search",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = SearchArgs,
	responses(
		(status = 200, body = Vec<DeviceInfo>),
	),
)]
pub async fn search(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SearchArgs>,
) -> Result<Json<Vec<DeviceInfo>>> {
	if args.query.trim().is_empty() {
		return Ok(Json(vec![]));
	}
	let mut conn = state.db.get().await?;
	let devices_by_key = Device::search_by_key(&mut conn, &args.query).await?;
	let devices_by_key_name = Device::search_by_key_name(&mut conn, &args.query).await?;
	let devices_by_ip = Device::search_by_connection_ip(&mut conn, &args.query).await?;
	let devices_by_tailscale =
		Device::search_by_tailscale_fields(&mut conn, &args.query).await?;

	// If the directory is configured, also resolve the query as a
	// Tailscale IP / node id / DNS name and surface any device
	// attached to the resolved node. This catches the case where the
	// operator pastes an identifier the device hasn't connected with
	// yet (so search_by_connection_ip would miss it).
	let directory_match = if let Some(directory) = &state.tailnet_directory {
		match directory.resolve_identifier(&args.query).await {
			Ok(Some(entry)) => {
				Device::get_with_info_by_node_id(&mut conn, &entry.node_id).await?
			}
			_ => None,
		}
	} else {
		None
	};

	let mut seen: HashMap<Uuid, DeviceWithInfo> = HashMap::new();
	for d in devices_by_key {
		seen.insert(d.device.id, d);
	}
	for d in devices_by_key_name {
		seen.insert(d.device.id, d);
	}
	for d in devices_by_ip {
		seen.insert(d.device.id, d);
	}
	for d in devices_by_tailscale {
		seen.insert(d.device.id, d);
	}
	if let Some(d) = directory_match {
		seen.insert(d.device.id, d);
	}

	// Enrich each result with live tailnet info.
	let mut out = Vec::with_capacity(seen.len());
	for d in seen.into_values() {
		out.push(DeviceInfo::from(d).enrich_with_live(&state).await);
	}
	Ok(Json(out))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateKeyNameArgs {
	pub key_id: Uuid,
	pub name: Option<String>,
}

#[utoipa::path(
	post,
	path = "/update_key_name",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = UpdateKeyNameArgs,
	responses(
		(status = 200),
	),
)]
pub async fn update_key_name(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<UpdateKeyNameArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	DeviceKey::update_name(&mut conn, args.key_id, args.name).await?;
	Ok(Json(()))
}

#[derive(Deserialize, ToSchema)]
pub struct AttachTailscaleArgs {
	pub device_id: Uuid,
	/// Any of: a Tailscale CGNAT/ULA IP, a node id, or a DNS name —
	/// the operator pastes whichever is most convenient from the
	/// Tailscale admin console. The server resolves it via the
	/// cached directory to the canonical `(node_id, name, tailnet)`
	/// tuple before persisting.
	pub identifier: String,
}

#[utoipa::path(
	post,
	path = "/attach_tailscale",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = AttachTailscaleArgs,
	responses(
		(status = 200, body = DeviceInfo),
		(status = 404, description = "Identifier does not resolve to a known tailnet node.", body = ProblemDetailsSchema),
		(status = 409, description = "Another device already claims this node id.", body = ProblemDetailsSchema),
		(status = 503, description = "Tailnet directory not configured or unreachable.", body = ProblemDetailsSchema),
	),
)]
pub async fn attach_tailscale(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<AttachTailscaleArgs>,
) -> Result<Json<DeviceInfo>> {
	let directory = state
		.tailnet_directory
		.as_ref()
		.ok_or(AppError::AuthTailnetDirectoryUnavailable)?;
	let entry = directory
		.resolve_identifier(&args.identifier)
		.await
		.map_err(|_| AppError::AuthTailnetDirectoryUnavailable)?
		.ok_or_else(|| AppError::custom("no tailnet device matches that identifier"))?;

	let mut conn = state.db.get().await?;
	Device::attach_tailscale(
		&mut conn,
		args.device_id,
		TailscaleIdentity {
			node_id: entry.node_id.clone(),
			node_name: Some(entry.node_name.clone()),
			tailnet: Some(entry.tailnet.clone()),
		},
	)
	.await?;

	let device_with_info = Device::get_with_info(&mut conn, args.device_id).await?;
	let info = DeviceInfo::from(device_with_info)
		.enrich_with_live(&state)
		.await;
	Ok(Json(info))
}

#[utoipa::path(
	post,
	path = "/detach_tailscale",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = DeviceIdArgs,
	responses(
		(status = 200, body = DeviceInfo),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn detach_tailscale(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<DeviceInfo>> {
	let mut conn = state.db.get().await?;
	Device::detach_tailscale(&mut conn, args.device_id).await?;
	let device_with_info = Device::get_with_info(&mut conn, args.device_id).await?;
	let info = DeviceInfo::from(device_with_info)
		.enrich_with_live(&state)
		.await;
	Ok(Json(info))
}

#[derive(Deserialize, ToSchema)]
pub struct MergeIntoArgs {
	/// Device row to fold *into* the target — usually the
	/// auto-discovered tailnet-only row.
	pub source_id: Uuid,
	/// Device row that keeps existing — usually the existing mTLS
	/// row that owns the device's server attachment and history.
	pub target_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/merge_into",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = MergeIntoArgs,
	responses(
		(status = 200, body = DeviceInfo),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, description = "Both source and target hold tailscale identity or a server attachment.", body = ProblemDetailsSchema),
	),
)]
pub async fn merge_into(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<MergeIntoArgs>,
) -> Result<Json<DeviceInfo>> {
	let mut conn = state.db.get().await?;
	Device::merge_into(&mut conn, args.source_id, args.target_id).await?;
	let device_with_info = Device::get_with_info(&mut conn, args.target_id).await?;
	let info = DeviceInfo::from(device_with_info)
		.enrich_with_live(&state)
		.await;
	Ok(Json(info))
}

#[derive(Deserialize, ToSchema)]
pub struct ResolveTailnetIdentifierArgs {
	pub identifier: String,
}

#[derive(Serialize, ToSchema)]
pub struct ResolveTailnetIdentifierResponse {
	pub matched: Option<TailnetLiveInfo>,
}

/// Look up a Tailscale IP / node id / DNS name in the directory and
/// return the canonical identity if found. Used by the attach UI's
/// preview pane so the operator can confirm the resolved node before
/// hitting attach.
#[utoipa::path(
	post,
	path = "/resolve_tailnet_identifier",
	tag = "devices",
	security(("tailscale-admin" = [])),
	request_body = ResolveTailnetIdentifierArgs,
	responses(
		(status = 200, body = ResolveTailnetIdentifierResponse),
		(status = 503, description = "Tailnet directory not configured or unreachable.", body = ProblemDetailsSchema),
	),
)]
pub async fn resolve_tailnet_identifier(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ResolveTailnetIdentifierArgs>,
) -> Result<Json<ResolveTailnetIdentifierResponse>> {
	let directory = state
		.tailnet_directory
		.as_ref()
		.ok_or(AppError::AuthTailnetDirectoryUnavailable)?;
	let matched = directory
		.resolve_identifier(&args.identifier)
		.await
		.map_err(|_| AppError::AuthTailnetDirectoryUnavailable)?
		.map(TailnetLiveInfo::from);
	Ok(Json(ResolveTailnetIdentifierResponse { matched }))
}
