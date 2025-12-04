use std::sync::Arc;

use commons_errors::Result;
use commons_types::{
	Uuid,
	geo::GeoPoint,
	server::{kind::ServerKind, rank::ServerRank},
	status::ShortStatus,
	version::VersionStr,
};
use jiff::Timestamp;
use leptos::serde_json::Value as JsonValue;
use leptos::server;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDetailData {
	pub server: Arc<ServerInfo>,
	pub device_info: Option<Arc<super::devices::DeviceInfo>>,
	pub last_status: Option<Arc<ServerLastStatusData>>,
	pub up: ShortStatus,
	pub child_servers: Vec<(ShortStatus, Arc<ServerInfo>)>,
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

#[server]
pub async fn count_some(kind: Option<ServerKind>) -> Result<u64> {
	ssr::count_some(kind).await
}

#[server]
pub async fn list_some(
	kind: Option<ServerKind>,
	offset: u64,
	limit: Option<u64>,
) -> Result<Vec<Arc<ServerInfo>>> {
	ssr::list_some(kind, offset, limit)
		.await
		.map(|v| v.into_iter().map(Arc::new).collect())
}

#[server]
pub async fn list_all() -> Result<Vec<ServerInfo>> {
	ssr::list_some(None, 0, None).await
}

#[server]
pub async fn list_centrals() -> Result<Vec<ServerInfo>> {
	ssr::list_some(Some(ServerKind::Central), 0, None).await
}

#[server]
pub async fn list_facilities() -> Result<Vec<ServerInfo>> {
	ssr::list_some(Some(ServerKind::Facility), 0, None).await
}

#[server]
pub async fn get_name(server_id: Uuid) -> Result<String> {
	ssr::get_name(server_id).await
}

#[server]
pub async fn get_info(server_id: Uuid) -> Result<ServerInfo> {
	ssr::get_info(server_id).await
}

#[server]
pub async fn get_detail(server_id: Uuid) -> Result<ServerDetailData> {
	ssr::get_detail(server_id).await
}

#[server(input = leptos::server_fn::codec::Json)]
pub async fn update(server_id: Uuid, data: ServerDataUpdate) -> Result<()> {
	ssr::update(server_id, data).await
}

#[server(input = leptos::server_fn::codec::Json)]
pub async fn search_parent(
	query: String,
	current_server_id: Uuid,
	current_rank: Option<ServerRank>,
	current_kind: ServerKind,
) -> Result<Vec<ServerInfo>> {
	ssr::search_parent(query, current_server_id, current_rank, current_kind).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use std::sync::Arc;

	use axum::extract::State;
	use commons_errors::{AppError, Result};

	use commons_types::server::{kind::ServerKind, rank::ServerRank};
	use database::{
		Db, Device, devices::DeviceConnection, servers::PartialServer, servers::Server,
		statuses::Status, url_field::UrlField, versions::Version,
	};
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;
	use uuid::Uuid;

	use crate::{fns::servers::ServerDataUpdate, state::AppState};

	pub async fn count_some(kind: Option<ServerKind>) -> Result<u64> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		if let Some(kind) = kind {
			Server::count_by_kind(&mut conn, kind).await
		} else {
			Server::count_all(&mut conn).await
		}
	}

	pub async fn list_some(
		kind: Option<ServerKind>,
		offset: u64,
		limit: Option<u64>,
	) -> Result<Vec<super::ServerInfo>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let servers = if let Some(kind) = kind {
			Server::list_by_kind(&mut conn, kind, offset, limit).await?
		} else {
			Server::get_all(&mut conn, offset, limit).await?
		};

		Ok(servers
			.into_iter()
			.map(|s| super::ServerInfo {
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
			})
			.collect())
	}

	pub async fn get_name(server_id: Uuid) -> Result<String> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		let server = Server::get_by_id(&mut conn, server_id).await?;
		Ok(server.name.unwrap_or_else(|| server.host.0.to_string()))
	}

	pub async fn get_info(server_id: Uuid) -> Result<super::ServerInfo> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let server = Server::get_by_id(&mut conn, server_id).await?;
		let device_id = server.device_id;

		let parent_server_name = if let Some(parent_id) = server.parent_server_id {
			let parent = Server::get_by_id(&mut conn, parent_id).await?;
			parent.name
		} else {
			None
		};

		Ok(super::ServerInfo {
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
		})
	}

	pub async fn get_detail(server_id: Uuid) -> Result<super::ServerDetailData> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let server = Server::get_by_id(&mut conn, server_id).await?;
		let device_id = server.device_id;

		let parent_server_name = if let Some(parent_id) = server.parent_server_id {
			let parent = Server::get_by_id(&mut conn, parent_id).await?;
			parent.name
		} else {
			None
		};

		let server_details = super::ServerInfo {
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

		let status = Status::latest_for_server(&mut conn, server.id).await?;
		let up = status
			.as_ref()
			.map(|s| s.short_status())
			.unwrap_or_default();

		let latest_version = Version::get_latest_matching(&mut conn, "*".parse()?)
			.await?
			.as_semver();

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
				compute_min_chrome_version(&state, &mut conn, version).await
			} else {
				None
			};

			Some(super::ServerLastStatusData {
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
			Some(convert_device_with_info_to_device_info(device_with_info))
		} else {
			None
		};

		// TODO: parallelise
		let mut child_servers = Vec::new();
		if server.kind.to_string() == "central" {
			let children = server.get_children(&mut conn).await?;
			for child in children {
				let child_status = Status::latest_for_server(&mut conn, child.id).await?;
				let child_up = child_status
					.as_ref()
					.map(|s| s.short_status())
					.unwrap_or_default();

				child_servers.push((
					child_up,
					Arc::new(super::ServerInfo {
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
					}),
				));
			}
		}

		Ok(super::ServerDetailData {
			server: Arc::new(server_details),
			device_info: device_info.map(Arc::new),
			last_status: last_status.map(Arc::new),
			up,
			child_servers,
		})
	}

	pub async fn update(server_id: Uuid, data: ServerDataUpdate) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let update_data = PartialServer {
			id: server_id,
			name: data.name,
			kind: data.kind,
			rank: data.rank,
			host: if let Some(host_str) = data.host {
				Some(UrlField(host_str.parse().map_err(|e| {
					AppError::custom(format!("Invalid URL: {}", e))
				})?))
			} else {
				None
			},
			device_id: data.device_id,
			parent_server_id: data.parent_server_id,
			listed: data.listed,
			cloud: data.cloud,
			geolocation: data.geolocation,
		};

		Server::update(&mut conn, server_id, update_data).await?;
		Ok(())
	}

	pub async fn search_parent(
		query: String,
		current_server_id: Uuid,
		current_rank: Option<ServerRank>,
		current_kind: ServerKind,
	) -> Result<Vec<super::ServerInfo>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let all_servers = Server::search_for_parent(
			&mut conn,
			&query,
			current_server_id,
			current_rank,
			current_kind,
		)
		.await?;

		Ok(all_servers
			.into_iter()
			.map(|s| super::ServerInfo {
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
			})
			.collect())
	}

	fn convert_device_with_info_to_device_info(
		device_with_info: database::DeviceWithInfo,
	) -> crate::fns::devices::DeviceInfo {
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

		crate::fns::devices::DeviceInfo {
			device: Arc::new(crate::fns::devices::DeviceData {
				id: device_with_info.device.id,
				created_at: device_with_info.device.created_at,
				updated_at: device_with_info.device.updated_at,
				role: device_with_info.device.role,
			}),
			keys: device_with_info
				.keys
				.into_iter()
				.map(|key| {
					Arc::new(crate::fns::devices::DeviceKeyInfo {
						id: key.id,
						device_id: key.device_id,
						name: key.name,
						pem_data: format_key_as_pem(&key.key_data),
						created_at: key.created_at,
					})
				})
				.collect(),
			latest_connection: device_with_info.latest_connection.map(|conn| {
				Arc::new(crate::fns::devices::DeviceConnectionData {
					id: conn.id,
					created_at: conn.created_at,
					device_id: conn.device_id,
					ip: conn.ip.addr().to_string(),
					user_agent: conn.user_agent,
				})
			}),
		}
	}

	async fn compute_min_chrome_version(
		state: &AppState,
		conn: &mut database::diesel_async::AsyncPgConnection,
		version: &commons_types::version::VersionStr,
	) -> Option<u32> {
		let head_release_date = Version::get_head_release_date(conn, version.clone())
			.await
			.ok()?;

		let supported_versions = state
			.chrome_cache
			.get_supported_versions_at_date(head_release_date)
			.await
			.ok()?;

		if supported_versions.is_empty() {
			return None;
		}

		let min = supported_versions.iter().min().copied()?;
		Some(min.saturating_sub(1))
	}
}
