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
	pub device_id: String,
	pub ip: String,
	pub user_agent: Option<String>,
}

#[server]
pub async fn list_untrusted_devices() -> Result<Vec<DeviceInfo>> {
	ssr::list_untrusted_devices().await
}

#[server]
pub async fn get_device_connection_history(
	device_id: String,
	limit: Option<i64>,
) -> Result<Vec<DeviceConnectionData>> {
	ssr::get_device_connection_history(device_id, limit).await
}

#[server]
pub async fn trust_device(device_id: String, role: String) -> Result<()> {
	ssr::trust_device(device_id, role).await
}

#[server]
pub async fn search_devices(query: String) -> Result<Vec<DeviceInfo>> {
	ssr::search_devices(query).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use database::{Device, DeviceConnection, DeviceKey, DeviceRole, DeviceWithInfo};
	use uuid::Uuid;

	impl From<DeviceWithInfo> for DeviceInfo {
		fn from(device_with_info: DeviceWithInfo) -> Self {
			Self {
				device: DeviceData {
					id: device_with_info.device.id.to_string(),
					created_at: device_with_info.device.created_at.to_string(),
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
				device_id: conn.device_id.to_string(),
				ip: conn.ip.to_string(),
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

	pub async fn list_untrusted_devices() -> Result<Vec<DeviceInfo>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let devices_with_info = Device::list_untrusted_with_info(&mut conn).await?;
		Ok(devices_with_info
			.into_iter()
			.map(DeviceInfo::from)
			.collect())
	}

	pub async fn get_device_connection_history(
		device_id: String,
		limit: Option<i64>,
	) -> Result<Vec<DeviceConnectionData>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let device_uuid = Uuid::parse_str(&device_id)
			.map_err(|_| commons_errors::AppError::custom("Invalid device ID"))?;

		let connections =
			DeviceConnection::get_history_for_device(&mut conn, device_uuid, limit.unwrap_or(50))
				.await?;
		Ok(connections
			.into_iter()
			.map(DeviceConnectionData::from)
			.collect())
	}

	pub async fn trust_device(device_id: String, role: String) -> Result<()> {
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

		Device::trust_device(&mut conn, device_uuid, device_role).await
	}

	pub async fn search_devices(query: String) -> Result<Vec<DeviceInfo>> {
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
