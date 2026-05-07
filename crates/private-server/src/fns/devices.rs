use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{Uuid, device::DeviceRole};
use database::devices::{Device, DeviceConnection, DeviceKey, DeviceWithInfo};
use database::servers::Server;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::fns::Page;
use crate::fns::servers::ServerInfo;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
	pub device: DeviceData,
	pub keys: Vec<DeviceKeyInfo>,
	pub latest_connection: Option<DeviceConnectionData>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceData {
	pub id: Uuid,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
	pub role: DeviceRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeyInfo {
	pub id: Uuid,
	pub device_id: Uuid,
	pub name: Option<String>,
	pub pem_data: String,
	pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
			},
			keys: d.keys.into_iter().map(DeviceKeyInfo::from).collect(),
			latest_connection: d.latest_connection.map(DeviceConnectionData::from),
		}
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
	}
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/get_device_by_id", post(get_device_by_id))
		.route("/list_untrusted", post(list_untrusted))
		.route("/get_servers_for_device", post(get_servers_for_device))
		.route("/get_past_server_associations", post(get_past_server_associations))
		.route("/connection_history", post(connection_history))
		.route("/connection_count", post(connection_count))
		.route("/trust", post(trust))
		.route("/list_trusted", post(list_trusted))
		.route("/untrust", post(untrust))
		.route("/update_role", post(update_role))
		.route("/search", post(search))
		.route("/update_key_name", post(update_key_name))
}

#[derive(Deserialize)]
pub struct DeviceIdArgs {
	pub device_id: Uuid,
}

pub async fn get_device_by_id(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<DeviceInfo>> {
	let mut conn = state.db.get().await?;
	let device_with_info = Device::get_with_info(&mut conn, args.device_id).await?;
	Ok(Json(DeviceInfo::from(device_with_info)))
}

#[derive(Deserialize)]
pub struct PaginationArgs {
	pub offset: u64,
	pub limit: Option<u64>,
}

pub async fn list_untrusted(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<PaginationArgs>,
) -> Result<Json<Page<DeviceInfo>>> {
	let mut conn = state.db.get().await?;
	let total = Device::count_untrusted(&mut conn).await?.try_into().unwrap_or(0);
	let devices_with_info = Device::list_untrusted_with_info_paginated(
		&mut conn,
		args.limit.unwrap_or(10).try_into().unwrap_or(10),
		args.offset.try_into().unwrap_or(0),
	)
	.await?;
	let items = devices_with_info.into_iter().map(DeviceInfo::from).collect();
	Ok(Json(Page { items, total }))
}

pub async fn get_servers_for_device(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<Vec<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let servers = Server::get_by_device_id(&mut conn, args.device_id).await?;
	Ok(Json(servers.into_iter().map(server_to_info).collect()))
}

pub async fn get_past_server_associations(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<Vec<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let servers = Server::get_past_associations_for_device(&mut conn, args.device_id).await?;
	Ok(Json(servers.into_iter().map(server_to_info).collect()))
}

#[derive(Deserialize)]
pub struct HistoryCursor {
	pub created_at: Timestamp,
	pub id: Uuid,
}

#[derive(Deserialize)]
pub struct ConnectionHistoryArgs {
	pub device_id: Uuid,
	pub before: Option<HistoryCursor>,
	pub limit: Option<u64>,
}

pub async fn connection_history(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
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

pub async fn connection_count(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
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

#[derive(Deserialize)]
pub struct TrustArgs {
	pub device_id: Uuid,
	pub role: DeviceRole,
}

pub async fn trust(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<TrustArgs>,
) -> Result<Json<()>> {
	if args.role == DeviceRole::Untrusted {
		return Err(AppError::custom("Cannot set device role to untrusted"));
	}
	let mut conn = state.db.get().await?;
	Device::trust(&mut conn, args.device_id, args.role).await?;
	Ok(Json(()))
}

pub async fn list_trusted(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<PaginationArgs>,
) -> Result<Json<Page<DeviceInfo>>> {
	let mut conn = state.db.get().await?;
	let total = Device::count_trusted(&mut conn).await?.try_into().unwrap_or(0);
	let devices_with_info = Device::list_trusted_with_info_paginated(
		&mut conn,
		args.limit.unwrap_or(10).try_into().unwrap_or(10),
		args.offset.try_into().unwrap_or(0),
	)
	.await?;
	let items = devices_with_info.into_iter().map(DeviceInfo::from).collect();
	Ok(Json(Page { items, total }))
}

pub async fn untrust(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<DeviceIdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Device::untrust(&mut conn, args.device_id).await?;
	Ok(Json(()))
}

pub async fn update_role(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
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

#[derive(Deserialize)]
pub struct SearchArgs {
	pub query: String,
}

pub async fn search(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<SearchArgs>,
) -> Result<Json<Vec<DeviceInfo>>> {
	if args.query.trim().is_empty() {
		return Ok(Json(vec![]));
	}
	let mut conn = state.db.get().await?;
	let devices_by_key = Device::search_by_key(&mut conn, &args.query).await?;
	let devices_by_key_name = Device::search_by_key_name(&mut conn, &args.query).await?;
	let devices_by_ip = Device::search_by_connection_ip(&mut conn, &args.query).await?;

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
	Ok(Json(
		seen.into_values()
			.map(DeviceInfo::from)
			
			.collect(),
	))
}

#[derive(Deserialize)]
pub struct UpdateKeyNameArgs {
	pub key_id: Uuid,
	pub name: Option<String>,
}

pub async fn update_key_name(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<UpdateKeyNameArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	DeviceKey::update_name(&mut conn, args.key_id, args.name).await?;
	Ok(Json(()))
}
