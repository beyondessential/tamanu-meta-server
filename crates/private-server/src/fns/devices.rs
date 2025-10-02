use commons_errors::Result;
use leptos::server;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
	pub device: DeviceData,
	pub keys: Vec<DeviceKeyInfo>,
	pub latest_connection: Option<DeviceConnectionData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceData {
	pub id: String,
	pub created_at: String,
	pub created_at_relative: String,
	pub updated_at: String,
	pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeyInfo {
	pub id: String,
	pub device_id: String,
	pub name: Option<String>,
	pub pem_data: String,
	pub hex_data: String,
	pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConnectionData {
	pub id: String,
	pub created_at: String,
	pub created_at_relative: String,
	pub device_id: String,
	pub ip: String,
	pub user_agent: Option<String>,
}

#[server]
pub async fn list_untrusted() -> Result<Vec<DeviceInfo>> {
	ssr::list_untrusted().await
}

#[server]
pub async fn connection_history(
	device_id: String,
	limit: Option<i64>,
	offset: Option<i64>,
) -> Result<Vec<DeviceConnectionData>> {
	ssr::connection_history(device_id, limit, offset).await
}

#[server]
pub async fn connection_count(device_id: String) -> Result<i64> {
	ssr::connection_count(device_id).await
}

#[server]
pub async fn trust(device_id: String, role: String) -> Result<()> {
	ssr::trust(device_id, role).await
}

#[server]
pub async fn list_trusted() -> Result<Vec<DeviceInfo>> {
	ssr::list_trusted().await
}

#[server]
pub async fn untrust(device_id: String) -> Result<()> {
	ssr::untrust(device_id).await
}

#[server]
pub async fn update_role(device_id: String, role: String) -> Result<()> {
	ssr::update_role(device_id, role).await
}

#[server]
pub async fn search(query: String) -> Result<Vec<DeviceInfo>> {
	ssr::search(query).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use database::{Device, DeviceConnection, DeviceKey, DeviceRole, DeviceWithInfo};
	use folktime::duration::Style;
	use uuid::Uuid;

	fn format_relative_time(datetime: chrono::DateTime<chrono::Utc>) -> String {
		let now = chrono::Utc::now();
		let duration = (now - datetime).to_std().unwrap_or_default();
		let relative = folktime::duration::Duration(duration, Style::OneUnitWhole);
		format!("{} ago", relative)
	}

	impl From<DeviceWithInfo> for DeviceInfo {
		fn from(device_with_info: DeviceWithInfo) -> Self {
			Self {
				device: DeviceData {
					id: device_with_info.device.id.to_string(),
					created_at: device_with_info.device.created_at.to_string(),
					created_at_relative: format_relative_time(device_with_info.device.created_at),
					updated_at: device_with_info.device.updated_at.to_string(),
					role: String::from(device_with_info.device.role),
				},
				keys: device_with_info
					.keys
					.into_iter()
					.map(DeviceKeyInfo::from)
					.collect(),
				latest_connection: device_with_info
					.latest_connection
					.map(DeviceConnectionData::from),
			}
		}
	}

	impl From<DeviceKey> for DeviceKeyInfo {
		fn from(key: DeviceKey) -> Self {
			let pem_data = format_key_as_pem(&key.key_data);
			let hex_data = format_key_as_hex(&key.key_data);

			Self {
				id: key.id.to_string(),
				device_id: key.device_id.to_string(),
				name: key.name,
				pem_data,
				hex_data,
				created_at: key.created_at.to_string(),
			}
		}
	}

	impl From<DeviceConnection> for DeviceConnectionData {
		fn from(conn: DeviceConnection) -> Self {
			Self {
				id: conn.id.to_string(),
				created_at: conn.created_at.to_string(),
				created_at_relative: format_relative_time(conn.created_at),
				device_id: conn.device_id.to_string(),
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

	fn format_key_as_hex(key_data: &[u8]) -> String {
		hex::encode(key_data)
			.chars()
			.collect::<Vec<_>>()
			.chunks(2)
			.map(|chunk| chunk.iter().collect::<String>())
			.collect::<Vec<_>>()
			.join(":")
			.to_uppercase()
	}

	pub async fn list_untrusted() -> Result<Vec<DeviceInfo>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let devices_with_info = Device::list_untrusted_with_info(&mut conn).await?;
		Ok(devices_with_info
			.into_iter()
			.map(DeviceInfo::from)
			.collect())
	}

	pub async fn list_trusted() -> Result<Vec<DeviceInfo>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let devices_with_info = Device::list_trusted_with_info(&mut conn).await?;
		Ok(devices_with_info
			.into_iter()
			.map(DeviceInfo::from)
			.collect())
	}

	pub async fn connection_history(
		device_id: String,
		limit: Option<i64>,
		offset: Option<i64>,
	) -> Result<Vec<DeviceConnectionData>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let device_uuid = Uuid::parse_str(&device_id)
			.map_err(|_| commons_errors::AppError::custom("Invalid device ID"))?;

		let connections = DeviceConnection::get_history_for_device_paginated(
			&mut conn,
			device_uuid,
			limit.unwrap_or(100),
			offset.unwrap_or(0),
		)
		.await?;
		Ok(connections
			.into_iter()
			.map(DeviceConnectionData::from)
			.collect())
	}

	pub async fn connection_count(device_id: String) -> Result<i64> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let device_uuid = Uuid::parse_str(&device_id)
			.map_err(|_| commons_errors::AppError::custom("Invalid device ID"))?;

		DeviceConnection::get_connection_count_for_device(&mut conn, device_uuid).await
	}

	pub async fn trust(device_id: String, role: String) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let device_uuid = Uuid::parse_str(&device_id)
			.map_err(|_| commons_errors::AppError::custom("Invalid device ID"))?;

		let device_role = DeviceRole::try_from(role)
			.map_err(|_| commons_errors::AppError::custom("Invalid device role"))?;

		// Prevent setting role to untrusted (that's the default for new devices)
		if device_role == DeviceRole::Untrusted {
			return Err(commons_errors::AppError::custom(
				"Cannot set device role to untrusted",
			));
		}

		Device::trust(&mut conn, device_uuid, device_role).await
	}

	pub async fn untrust(device_id: String) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let device_uuid = Uuid::parse_str(&device_id)
			.map_err(|_| commons_errors::AppError::custom("Invalid device ID"))?;

		Device::untrust(&mut conn, device_uuid).await
	}

	pub async fn update_role(device_id: String, role: String) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let device_uuid = Uuid::parse_str(&device_id)
			.map_err(|_| commons_errors::AppError::custom("Invalid device ID"))?;

		let device_role = DeviceRole::try_from(role)
			.map_err(|_| commons_errors::AppError::custom("Invalid device role"))?;

		// Prevent setting role to untrusted (use untrust function instead)
		if device_role == DeviceRole::Untrusted {
			return Err(commons_errors::AppError::custom(
				"Use untrust function to set device role to untrusted",
			));
		}

		Device::trust(&mut conn, device_uuid, device_role).await
	}

	pub async fn search(query: String) -> Result<Vec<DeviceInfo>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		if query.trim().is_empty() {
			return Ok(vec![]);
		}

		let devices_with_info = Device::search_by_key(&mut conn, &query).await?;
		Ok(devices_with_info
			.into_iter()
			.map(DeviceInfo::from)
			.collect())
	}
}
