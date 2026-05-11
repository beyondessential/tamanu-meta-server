use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::server::CanopyTicket;
use commons_types::{
	Uuid,
	geo::GeoPoint,
	server::{kind::ServerKind, rank::ServerRank},
	status::ShortStatus,
	version::VersionStr,
};
use database::{
	devices::{Device, DeviceConnection, DeviceWithInfo},
	servers::{PartialServer, Server},
	statuses::Status,
	url_field::UrlField,
	versions::Version,
};
use futures::future::join;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::fns::Page;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDetailData {
	pub server: ServerInfo,
	pub device_info: Option<super::devices::DeviceInfo>,
	pub last_status: Option<ServerLastStatusData>,
	pub up: ShortStatus,
	pub child_servers: Vec<(ShortStatus, ServerInfo)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
	pub id: Uuid,
	pub name: Option<String>,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
	pub host: String,
	pub device_id: Option<Uuid>,
	pub parent_server_id: Option<Uuid>,
	pub parent_server_name: Option<String>,
	pub listed: bool,
	pub cloud: Option<bool>,
	pub geolocation: Option<GeoPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
	pub extra: JsonValue,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
	pub parent_server_id: Option<Option<Uuid>>,
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
}

fn deserialize_some<'de, T, D>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
	T: Deserialize<'de>,
	D: serde::Deserializer<'de>,
{
	Deserialize::deserialize(deserializer).map(Some)
}

fn server_to_info(s: Server) -> ServerInfo {
	ServerInfo {
		id: s.id,
		name: s.name,
		kind: s.kind,
		rank: s.rank,
		host: s.host.0.to_string(),
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
		.route("/list_some", post(list_some))
		.route("/list_roots", post(list_roots))
		.route("/get_name", post(get_name))
		.route("/get_info", post(get_info))
		.route("/get_detail", post(get_detail))
		.route("/update", post(update))
		.route("/import_ticket", post(import_ticket))
		.route("/search_parent", post(search_parent))
}

/// Root servers — those without a parent. Each one heads a server-group
/// (the unit incidents roll up to). Used by the Incidents page filter.
pub async fn list_roots(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(_): Json<serde_json::Value>,
) -> Result<Json<Vec<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let servers = Server::list_roots(&mut conn).await?;
	Ok(Json(servers.into_iter().map(server_to_info).collect()))
}

#[derive(Deserialize)]
pub struct ListArgs {
	pub kind: Option<ServerKind>,
	pub offset: u64,
	pub limit: Option<u64>,
}

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
	let items = servers.into_iter().map(server_to_info).collect();
	Ok(Json(Page { items, total }))
}

#[derive(Deserialize)]
pub struct ServerIdArgs {
	pub server_id: Uuid,
}

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

pub async fn get_info(
	State(state): State<AppState>,
	Json(args): Json<ServerIdArgs>,
) -> Result<Json<ServerInfo>> {
	let db = state.db;
	let mut conn = db.get().await?;
	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	let parent_server_name = if let Some(parent_id) = server.parent_server_id {
		Server::get_by_id(&mut conn, parent_id).await?.name
	} else {
		None
	};
	Ok(Json(ServerInfo {
		id: server.id,
		name: server.name.clone(),
		kind: server.kind,
		rank: server.rank,
		host: server.host.0.to_string(),
		device_id: server.device_id,
		parent_server_id: server.parent_server_id,
		parent_server_name,
		listed: server.listed,
		cloud: server.cloud,
		geolocation: server.geolocation,
	}))
}

pub async fn get_detail(
	State(state): State<AppState>,
	Json(args): Json<ServerIdArgs>,
) -> Result<Json<ServerDetailData>> {
	let db = state.db.clone();
	let mut conn = db.get().await?;
	let server = Server::get_by_id(&mut conn, args.server_id).await?;
	let device_id = server.device_id;

	let (parent_server_name, status, latest_version) = {
		let mut conn_parent = db.get().await?;
		let mut conn_status = db.get().await?;

		let parent_lookup = async {
			if let Some(parent_id) = server.parent_server_id {
				let parent = Server::get_by_id(&mut conn_parent, parent_id).await?;
				Ok::<_, AppError>(parent.name)
			} else {
				Ok(None)
			}
		};

		let status_lookup = Status::latest_for_server(&mut conn_status, server.id);

		let latest_version_lookup = async {
			Version::get_latest_matching(&mut conn, "*".parse()?)
				.await
				.map(|v| v.as_semver())
		};

		let (parent_result, status_result) = join(parent_lookup, status_lookup).await;
		(parent_result?, status_result?, latest_version_lookup.await?)
	};

	let server_details = ServerInfo {
		id: server.id,
		name: server.name.clone(),
		kind: server.kind,
		rank: server.rank,
		host: server.host.0.to_string(),
		device_id,
		parent_server_id: server.parent_server_id,
		parent_server_name,
		listed: server.listed,
		cloud: server.cloud,
		geolocation: server.geolocation,
	};

	let up = status
		.as_ref()
		.map(|s| s.short_status())
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
			extra: st.extra.clone(),
		})
	} else {
		None
	};

	let device_info = if let Some(device_id) = device_id {
		let device_with_info = Device::get_with_info(&mut conn, device_id).await?;
		Some(convert_device_with_info(device_with_info))
	} else {
		None
	};

	let child_servers = if server.kind == ServerKind::Central {
		let children = server.get_children(&mut conn).await?;
		if children.is_empty() {
			Vec::new()
		} else {
			let child_ids: Vec<Uuid> = children.iter().map(|c| c.id).collect();
			let statuses = Status::latest_for_servers(&mut conn, &child_ids).await?;
			let status_map: std::collections::HashMap<Uuid, &Status> =
				statuses.iter().map(|s| (s.server_id, s)).collect();

			children
				.into_iter()
				.map(|child| {
					let child_status = status_map.get(&child.id).copied();
					let child_up = child_status.map(|s| s.short_status()).unwrap_or_default();
					(
						child_up,
						ServerInfo {
							id: child.id,
							name: child.name,
							kind: child.kind,
							rank: child.rank,
							host: child.host.0.to_string(),
							listed: child.listed,
							cloud: child.cloud,
							geolocation: child.geolocation,
							device_id: child.device_id,
							parent_server_id: Some(server.id),
							parent_server_name: server.name.clone(),
						},
					)
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
		child_servers,
	}))
}

#[derive(Deserialize)]
pub struct UpdateArgs {
	pub server_id: Uuid,
	pub data: ServerDataUpdate,
}

pub async fn update(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<UpdateArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
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
		parent_server_id: args.data.parent_server_id,
		listed: args.data.listed,
		cloud: args.data.cloud,
		geolocation: args.data.geolocation,
	};
	Server::update(&mut conn, args.server_id, update_data).await?;
	Ok(Json(()))
}

#[derive(Deserialize)]
pub struct ImportTicketArgs {
	pub ticket_b64: String,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
}

pub async fn import_ticket(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<ImportTicketArgs>,
) -> Result<Json<Uuid>> {
	let mut conn = state.db.get().await?;
	let ticket = CanopyTicket::from_base64(&args.ticket_b64)?;
	let server = Server::upsert_from_ticket(&mut conn, &ticket, args.kind, args.rank).await?;
	Ok(Json(server.id))
}

#[derive(Deserialize)]
pub struct SearchParentArgs {
	pub query: String,
	pub current_server_id: Uuid,
	pub current_rank: Option<ServerRank>,
	pub current_kind: ServerKind,
}

pub async fn search_parent(
	State(state): State<AppState>,
	Json(args): Json<SearchParentArgs>,
) -> Result<Json<Vec<ServerInfo>>> {
	let mut conn = state.db.get().await?;
	let all_servers = Server::search_for_parent(
		&mut conn,
		&args.query,
		args.current_server_id,
		args.current_rank,
		args.current_kind,
	)
	.await?;
	Ok(Json(all_servers.into_iter().map(server_to_info).collect()))
}

fn convert_device_with_info(d: DeviceWithInfo) -> super::devices::DeviceInfo {
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

	super::devices::DeviceInfo {
		device: super::devices::DeviceData {
			id: d.device.id,
			created_at: d.device.created_at,
			updated_at: d.device.updated_at,
			role: d.device.role,
		},
		keys: d
			.keys
			.into_iter()
			.map(|key| super::devices::DeviceKeyInfo {
				id: key.id,
				device_id: key.device_id,
				name: key.name,
				pem_data: format_key_as_pem(&key.key_data),
				created_at: key.created_at,
			})
			.collect(),
		latest_connection: d
			.latest_connection
			.map(|conn| super::devices::DeviceConnectionData {
				id: conn.id,
				created_at: conn.created_at,
				device_id: conn.device_id,
				ip: conn.ip.addr().to_string(),
				user_agent: conn.user_agent,
			}),
	}
}

async fn compute_min_chrome_version(
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
