use std::sync::Arc;

use commons_errors::Result;
use commons_types::{
	Uuid,
	device::DeviceRole,
	server::{kind::ServerKind, rank::ServerRank},
};
use jiff::Timestamp;
use leptos::server;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
	pub id: Uuid,
	pub name: Option<String>,
	pub host: String,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
	pub device: Arc<DeviceData>,
	pub keys: Vec<Arc<DeviceKeyInfo>>,
	pub latest_connection: Option<Arc<DeviceConnectionData>>,
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

#[server]
pub async fn get_device_by_id(device_id: Uuid) -> Result<DeviceInfo> {
	ssr::get_device_by_id(device_id).await
}

#[server]
pub async fn list_untrusted(
	limit: Option<i64>,
	offset: Option<i64>,
) -> Result<Vec<Arc<DeviceInfo>>> {
	ssr::list_untrusted(limit, offset).await
}

#[server]
pub async fn get_servers_for_device(device_id: Uuid) -> Result<Vec<ServerInfo>> {
	ssr::get_servers_for_device(device_id).await
}

#[server]
pub async fn count_untrusted() -> Result<i64> {
	ssr::count_untrusted().await
}

#[server]
pub async fn connection_history(
	device_id: Uuid,
	limit: Option<i64>,
	offset: Option<i64>,
) -> Result<Vec<DeviceConnectionData>> {
	ssr::connection_history(device_id, limit, offset).await
}

#[server]
pub async fn connection_count(device_id: Uuid) -> Result<i64> {
	ssr::connection_count(device_id).await
}

#[server]
pub async fn trust(device_id: Uuid, role: DeviceRole) -> Result<()> {
	ssr::trust(device_id, role).await
}

#[server]
pub async fn list_trusted(limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Arc<DeviceInfo>>> {
	ssr::list_trusted(limit, offset).await
}

#[server]
pub async fn count_trusted() -> Result<i64> {
	ssr::count_trusted().await
}

#[server]
pub async fn untrust(device_id: Uuid) -> Result<()> {
	ssr::untrust(device_id).await
}

#[server]
pub async fn update_role(device_id: Uuid, role: DeviceRole) -> Result<()> {
	ssr::update_role(device_id, role).await
}

#[server]
pub async fn search(query: String) -> Result<Vec<Arc<DeviceInfo>>> {
	ssr::search(query).await
}

#[server]
pub async fn update_key_name(key_id: Uuid, name: Option<String>) -> Result<()> {
	ssr::update_key_name(key_id, name).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use commons_types::device::DeviceRole;
	use database::servers::Server;
	use database::{Device, DeviceConnection, DeviceKey, DeviceWithInfo};
	use uuid::Uuid;

	impl From<DeviceWithInfo> for DeviceInfo {
		fn from(device_with_info: DeviceWithInfo) -> Self {
			Self {
				device: Arc::new(DeviceData {
					id: device_with_info.device.id,
					created_at: device_with_info.device.created_at,
					updated_at: device_with_info.device.updated_at,
					role: device_with_info.device.role,
				}),
				keys: device_with_info
					.keys
					.into_iter()
					.map(DeviceKeyInfo::from)
					.map(Arc::new)
					.collect(),
				latest_connection: device_with_info
					.latest_connection
					.map(DeviceConnectionData::from)
					.map(Arc::new),
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

		// Split into 64-character lines
		for chunk in base64_data.as_bytes().chunks(64) {
			pem.push_str(&String::from_utf8_lossy(chunk));
			pem.push('\n');
		}

		pem.push_str("-----END PUBLIC KEY-----");
		pem
	}

	pub async fn get_device_by_id(device_id: Uuid) -> Result<DeviceInfo> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let device_with_info = Device::get_with_info(&mut conn, device_id).await?;
		Ok(DeviceInfo::from(device_with_info))
	}

	pub async fn get_servers_for_device(device_id: Uuid) -> Result<Vec<ServerInfo>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let servers = Server::get_by_device_id(&mut conn, device_id).await?;
		Ok(servers
			.into_iter()
			.map(|s| ServerInfo {
				id: s.id,
				name: s.name,
				host: s.host.into(),
				kind: s.kind,
				rank: s.rank,
			})
			.collect())
	}

	pub async fn list_untrusted(
		limit: Option<i64>,
		offset: Option<i64>,
	) -> Result<Vec<Arc<DeviceInfo>>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let devices_with_info = Device::list_untrusted_with_info_paginated(
			&mut conn,
			limit.unwrap_or(10),
			offset.unwrap_or(0),
		)
		.await?;
		Ok(devices_with_info
			.into_iter()
			.map(DeviceInfo::from)
			.map(Arc::new)
			.collect())
	}

	pub async fn count_untrusted() -> Result<i64> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		Device::count_untrusted(&mut conn).await
	}

	pub async fn list_trusted(
		limit: Option<i64>,
		offset: Option<i64>,
	) -> Result<Vec<Arc<DeviceInfo>>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let devices_with_info = Device::list_trusted_with_info_paginated(
			&mut conn,
			limit.unwrap_or(10),
			offset.unwrap_or(0),
		)
		.await?;
		Ok(devices_with_info
			.into_iter()
			.map(DeviceInfo::from)
			.map(Arc::new)
			.collect())
	}

	pub async fn count_trusted() -> Result<i64> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		Device::count_trusted(&mut conn).await
	}

	pub async fn connection_history(
		device_id: Uuid,
		limit: Option<i64>,
		offset: Option<i64>,
	) -> Result<Vec<DeviceConnectionData>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let connections = DeviceConnection::get_history_for_device_paginated(
			&mut conn,
			device_id,
			limit.unwrap_or(100),
			offset.unwrap_or(0),
		)
		.await?;
		Ok(connections
			.into_iter()
			.map(DeviceConnectionData::from)
			.collect())
	}

	pub async fn connection_count(device_id: Uuid) -> Result<i64> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		DeviceConnection::get_connection_count_for_device(&mut conn, device_id).await
	}

	pub async fn trust(device_id: Uuid, role: DeviceRole) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		// Prevent setting role to untrusted (that's the default for new devices)
		if role == DeviceRole::Untrusted {
			return Err(commons_errors::AppError::custom(
				"Cannot set device role to untrusted",
			));
		}

		Device::trust(&mut conn, device_id, role).await
	}

	pub async fn untrust(device_id: Uuid) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		Device::untrust(&mut conn, device_id).await
	}

	pub async fn update_role(device_id: Uuid, role: DeviceRole) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		// Prevent setting role to untrusted (use untrust function instead)
		if role == DeviceRole::Untrusted {
			return Err(commons_errors::AppError::custom(
				"Use untrust function to set device role to untrusted",
			));
		}

		Device::trust(&mut conn, device_id, role).await
	}

	pub async fn search(query: String) -> Result<Vec<Arc<DeviceInfo>>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		if query.trim().is_empty() {
			return Ok(vec![]);
		}

		let devices_with_info = Device::search_by_key(&mut conn, &query).await?;
		Ok(devices_with_info
			.into_iter()
			.map(DeviceInfo::from)
			.map(Arc::new)
			.collect())
	}

	pub async fn update_key_name(key_id: Uuid, name: Option<String>) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		DeviceKey::update_name(&mut conn, key_id, name).await
	}
}
