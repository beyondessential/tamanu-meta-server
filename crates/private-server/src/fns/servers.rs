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
pub struct ServerDetailsData {
	pub id: Uuid,
	pub name: String,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
	pub host: String,
	pub parent_server_id: Option<Uuid>,
	pub parent_server_name: Option<String>,
	pub listed: bool,
	pub cloud: Option<bool>,
	pub geolocation: Option<GeoPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
	pub id: Uuid,
	pub name: Option<String>,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
	pub host: String,
	pub parent_server_id: Option<Uuid>,
	pub parent_server_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDetailData {
	pub server: Arc<ServerDetailsData>,
	pub device_info: Option<Arc<super::devices::DeviceInfo>>,
	pub last_status: Option<Arc<ServerLastStatusData>>,
	pub up: ShortStatus,
	pub child_servers: Vec<Arc<ChildServerData>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildServerData {
	pub id: Uuid,
	pub name: String,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
	pub host: String,
	pub up: ShortStatus,
	pub last_status: Option<Arc<ServerLastStatusData>>,
	pub device_info: Option<Arc<super::devices::DeviceInfo>>,
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

#[server]
pub async fn count_servers(kind: Option<ServerKind>) -> Result<u64> {
	ssr::count_servers(kind).await
}

#[server]
pub async fn list_servers(
	kind: Option<ServerKind>,
	offset: u64,
	limit: Option<u64>,
) -> Result<Vec<Arc<ServerInfo>>> {
	ssr::list_servers(kind, offset, limit)
		.await
		.map(|v| v.into_iter().map(Arc::new).collect())
}

#[server]
pub async fn list_all_servers() -> Result<Vec<ServerInfo>> {
	ssr::list_servers(None, 0, None).await
}

#[server]
pub async fn list_central_servers() -> Result<Vec<ServerInfo>> {
	ssr::list_servers(Some(ServerKind::Central), 0, None).await
}

#[server]
pub async fn list_facility_servers() -> Result<Vec<ServerInfo>> {
	ssr::list_servers(Some(ServerKind::Facility), 0, None).await
}

#[server]
pub async fn server_detail(server_id: Uuid) -> Result<ServerDetailData> {
	ssr::server_detail(server_id).await
}

#[server]
pub async fn update_server(
	server_id: Uuid,
	name: Option<String>,
	host: Option<String>,
	rank: Option<ServerRank>,
	device_id: Option<Uuid>,
	parent_server_id: Option<Uuid>,
	listed: Option<bool>,
	cloud: Option<Option<bool>>,
	geolocation: Option<Option<GeoPoint>>,
) -> Result<ServerDetailsData> {
	ssr::update_server(
		server_id,
		name,
		host,
		rank,
		device_id,
		parent_server_id,
		listed,
		cloud,
		geolocation,
	)
	.await
}

#[server]
pub async fn search_central_servers(query: String) -> Result<Vec<ServerInfo>> {
	ssr::search_central_servers(query).await
}

#[server]
pub async fn assign_parent_server(
	server_id: Uuid,
	parent_server_id: Uuid,
) -> Result<ServerDetailsData> {
	ssr::assign_parent_server(server_id, parent_server_id).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use std::sync::Arc;

	use axum::extract::State;
	use commons_errors::{AppError, Result};

	use commons_types::{
		geo::GeoPoint,
		server::{kind::ServerKind, rank::ServerRank},
	};
	use database::{
		Db, Device, devices::DeviceConnection, servers::PartialServer, servers::Server,
		statuses::Status, url_field::UrlField, versions::Version,
	};
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;
	use uuid::Uuid;

	use crate::state::AppState;

	pub async fn count_servers(kind: Option<ServerKind>) -> Result<u64> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		if let Some(kind) = kind {
			Server::count_by_kind(&mut conn, kind).await
		} else {
			Server::count_all(&mut conn).await
		}
	}

	pub async fn list_servers(
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
				parent_server_id: s.parent_server_id,
				parent_server_name: None,
			})
			.collect())
	}

	pub async fn server_detail(server_id: Uuid) -> Result<super::ServerDetailData> {
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

		let server_details = super::ServerDetailsData {
			id: server.id,
			name: server.name.clone().unwrap_or_default(),
			kind: server.kind,
			rank: server.rank,
			host: server.host.0.to_string(),
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
			let nodejs = device.map(|d| d.nodejs_version()).flatten();

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

		let child_servers = if server.kind.to_string() == "central" {
			let children = server.get_children(&mut conn).await?;
			let mut child_data = Vec::new();

			for child in children {
				let child_device_info = if let Some(device_id) = child.device_id {
					let device_with_info = Device::get_with_info(&mut conn, device_id).await?;
					Some(convert_device_with_info_to_device_info(device_with_info))
				} else {
					None
				};

				let child_status = Status::latest_for_server(&mut conn, child.id).await?;
				let child_up = child_status
					.as_ref()
					.map(|s| s.short_status())
					.unwrap_or_default();

				let child_last_status = if let Some(st) = child_status.as_ref() {
					let device = if let Some(device_id) = st.device_id {
						DeviceConnection::get_latest_from_device_ids(
							&mut conn,
							[device_id].into_iter(),
						)
						.await?
						.into_iter()
						.next()
					} else {
						None
					};

					let platform = st.extra("pgVersion").and_then(|pg| pg.as_str()).map(|pg| {
						if pg.contains("Visual C++") || pg.contains("windows") {
							"Windows"
						} else {
							"Linux"
						}
						.into()
					});

					let postgres = st
						.extra("pgVersion")
						.and_then(|pg| pg.as_str())
						.and_then(|pg| pg.split_ascii_whitespace().nth(1))
						.map(|vers| vers.trim_end_matches(',').into());

					let nodejs = device
						.as_ref()
						.and_then(|d| d.user_agent.as_ref())
						.and_then(|ua| {
							ua.split_ascii_whitespace()
								.find_map(|p| p.strip_prefix("Node.js/"))
								.map(ToOwned::to_owned)
						});

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

				child_data.push(Arc::new(super::ChildServerData {
					id: child.id,
					name: child.name.unwrap_or_default(),
					kind: child.kind,
					rank: child.rank,
					host: child.host.0.to_string(),
					up: child_up,
					last_status: child_last_status.map(Arc::new),
					device_info: child_device_info.map(Arc::new),
				}));
			}
			child_data
		} else {
			Vec::new()
		};

		Ok(super::ServerDetailData {
			server: Arc::new(server_details),
			device_info: device_info.map(Arc::new),
			last_status: last_status.map(Arc::new),
			up,
			child_servers,
		})
	}

	pub async fn update_server(
		server_id: Uuid,
		name: Option<String>,
		host: Option<String>,
		rank: Option<ServerRank>,
		device_id: Option<Uuid>,
		parent_server_id: Option<Uuid>,
		listed: Option<bool>,
		cloud: Option<Option<bool>>,
		geolocation: Option<Option<GeoPoint>>,
	) -> Result<super::ServerDetailsData> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let parsed_host = if let Some(host_str) = host {
			Some(UrlField(host_str.parse().map_err(|e| {
				AppError::custom(format!("Invalid URL: {}", e))
			})?))
		} else {
			None
		};

		let update_data = PartialServer {
			id: server_id,
			name,
			kind: None,
			rank,
			host: parsed_host,
			device_id: Some(device_id),
			parent_server_id: Some(parent_server_id),
			listed,
			cloud,
			geolocation,
		};

		let server = Server::update(&mut conn, server_id, update_data).await?;

		let parent_server_name = if let Some(parent_id) = server.parent_server_id {
			let parent = Server::get_by_id(&mut conn, parent_id).await?;
			parent.name
		} else {
			None
		};

		Ok(super::ServerDetailsData {
			id: server.id,
			name: server.name.unwrap_or_default(),
			kind: server.kind,
			rank: server.rank,
			host: server.host.0.to_string(),
			parent_server_id: server.parent_server_id,
			parent_server_name,
			listed: server.listed,
			cloud: server.cloud,
			geolocation: server.geolocation,
		})
	}

	pub async fn search_central_servers(query: String) -> Result<Vec<super::ServerInfo>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let servers = Server::search_central(&mut conn, &query, 10).await?;

		Ok(servers
			.into_iter()
			.map(|s| super::ServerInfo {
				id: s.id,
				name: s.name,
				kind: s.kind,
				rank: s.rank,
				host: s.host.0.to_string(),
				parent_server_id: s.parent_server_id,
				parent_server_name: None,
			})
			.collect())
	}

	pub async fn assign_parent_server(
		id: Uuid,
		parent_id: Uuid,
	) -> Result<super::ServerDetailsData> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		// Verify parent is a central server
		let parent = Server::get_by_id(&mut conn, parent_id).await?;
		if parent.kind != ServerKind::Central {
			return Err(AppError::custom("Parent server must be of kind 'central'"));
		}

		let update_data = PartialServer {
			id,
			name: None,
			kind: None,
			rank: None,
			host: None,
			device_id: None,
			parent_server_id: Some(Some(parent_id)),
			listed: None,
			cloud: None,
			geolocation: None,
		};

		let server = Server::update(&mut conn, id, update_data).await?;

		let parent_server_name = if let Some(parent_id) = server.parent_server_id {
			let parent = Server::get_by_id(&mut conn, parent_id).await?;
			parent.name
		} else {
			None
		};

		Ok(super::ServerDetailsData {
			id: server.id,
			name: server.name.unwrap_or_default(),
			kind: server.kind,
			rank: server.rank,
			host: server.host.0.to_string(),
			parent_server_id: server.parent_server_id,
			parent_server_name,
			listed: server.listed,
			cloud: server.cloud,
			geolocation: server.geolocation,
		})
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
