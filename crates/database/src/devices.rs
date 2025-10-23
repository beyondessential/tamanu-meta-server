use base64::Engine;
use chrono::{DateTime, Utc};
use commons_errors::{AppError, Result};
use diesel::QueryableByName;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::device_role::DeviceRole;

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Device {
	/// The ID of the device.
	pub id: Uuid,

	/// The created timestamp.
	pub created_at: DateTime<Utc>,

	/// The updated timestamp.
	pub updated_at: DateTime<Utc>,

	/// The role of the device.
	///
	/// This is used for permission checks.
	pub role: DeviceRole,
}

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::device_keys)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DeviceKey {
	/// The ID of the device key.
	pub id: Uuid,

	/// The created timestamp.
	pub created_at: DateTime<Utc>,

	/// The updated timestamp.
	pub updated_at: DateTime<Utc>,

	/// The device this key belongs to.
	pub device_id: Uuid,

	/// The public key data in PublicKeyInfo form.
	///
	/// This is the RFC 5280, Section 4.1.2.7 form of the public key as contained by X.509
	/// certificates or by RFC 7250 Raw Public Keys.
	///
	/// This contains both the public key and its algorithm, and is extensible to support all types
	/// of keys that TLS or X.509 in general can support.
	pub key_data: Vec<u8>,

	/// Optional name/description for the key.
	pub name: Option<String>,

	/// Whether this key is active and can be used for authentication.
	pub is_active: bool,
}

/// Device with its keys and latest connection info for management purposes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceWithInfo {
	pub device: Device,
	pub keys: Vec<DeviceKey>,
	pub latest_connection: Option<DeviceConnection>,
}

impl Device {
	pub async fn from_key(db: &mut AsyncPgConnection, key: &[u8]) -> Result<Option<Self>> {
		use crate::schema::{device_keys, devices};

		devices::table
			.inner_join(device_keys::table.on(device_keys::device_id.eq(devices::id)))
			.select(Self::as_select())
			.filter(device_keys::key_data.eq(key))
			.filter(device_keys::is_active.eq(true))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	pub async fn create(db: &mut AsyncPgConnection, key: Vec<u8>) -> Result<Self> {
		use crate::schema::devices;

		// Create the device first
		let device: Self = diesel::insert_into(devices::table)
			.default_values()
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)?;

		// Create the initial key for the device
		DeviceKey::create(db, device.id, key, Some("Initial Key".to_string())).await?;

		Ok(device)
	}

	/// Get a single device by ID with its keys and latest connection info.
	pub async fn get_with_info(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
	) -> Result<DeviceWithInfo> {
		use crate::schema::{device_keys, devices};

		let device: Self = devices::table
			.select(Self::as_select())
			.filter(devices::id.eq(device_id))
			.first(db)
			.await
			.map_err(AppError::from)?;

		let keys: Vec<DeviceKey> = device_keys::table
			.select(DeviceKey::as_select())
			.filter(device_keys::device_id.eq(device_id))
			.filter(device_keys::is_active.eq(true))
			.order(device_keys::created_at.asc())
			.load(db)
			.await
			.map_err(AppError::from)?;

		let latest_connections =
			DeviceConnection::get_latest_from_device_ids(db, std::iter::once(device_id)).await?;

		let latest_connection = latest_connections.into_iter().next();

		Ok(DeviceWithInfo {
			device,
			keys,
			latest_connection,
		})
	}

	/// List all untrusted devices with their keys and latest connection info.
	pub async fn list_untrusted_with_info(
		db: &mut AsyncPgConnection,
	) -> Result<Vec<DeviceWithInfo>> {
		Self::list_untrusted_with_info_paginated(db, i64::MAX, 0).await
	}

	/// List untrusted devices with pagination.
	pub async fn list_untrusted_with_info_paginated(
		db: &mut AsyncPgConnection,
		limit: i64,
		offset: i64,
	) -> Result<Vec<DeviceWithInfo>> {
		use crate::schema::{device_keys, devices};

		let untrusted_devices: Vec<Self> = devices::table
			.select(Self::as_select())
			.filter(devices::role.eq(DeviceRole::Untrusted))
			.order(devices::created_at.desc())
			.limit(limit)
			.offset(offset)
			.load(db)
			.await
			.map_err(AppError::from)?;

		let device_ids: Vec<Uuid> = untrusted_devices.iter().map(|d| d.id).collect();

		let device_keys: Vec<DeviceKey> = device_keys::table
			.select(DeviceKey::as_select())
			.filter(device_keys::device_id.eq_any(&device_ids))
			.filter(device_keys::is_active.eq(true))
			.order(device_keys::created_at.asc())
			.load(db)
			.await
			.map_err(AppError::from)?;

		let latest_connections =
			DeviceConnection::get_latest_from_device_ids(db, device_ids.iter().copied()).await?;

		let mut keys_by_device: HashMap<Uuid, Vec<DeviceKey>> = HashMap::new();
		for key in device_keys {
			keys_by_device.entry(key.device_id).or_default().push(key);
		}

		let mut connections_by_device: HashMap<Uuid, DeviceConnection> = HashMap::new();
		for connection in latest_connections {
			connections_by_device.insert(connection.device_id, connection);
		}

		let result = untrusted_devices
			.into_iter()
			.map(|device| DeviceWithInfo {
				keys: keys_by_device.remove(&device.id).unwrap_or_default(),
				latest_connection: connections_by_device.remove(&device.id),
				device,
			})
			.collect();

		Ok(result)
	}

	/// List all trusted devices with their keys and latest connection info.
	pub async fn list_trusted_with_info(db: &mut AsyncPgConnection) -> Result<Vec<DeviceWithInfo>> {
		Self::list_trusted_with_info_paginated(db, i64::MAX, 0).await
	}

	/// List trusted devices with pagination.
	pub async fn list_trusted_with_info_paginated(
		db: &mut AsyncPgConnection,
		limit: i64,
		offset: i64,
	) -> Result<Vec<DeviceWithInfo>> {
		use crate::schema::{device_keys, devices};

		let trusted_devices: Vec<Self> = devices::table
			.select(Self::as_select())
			.filter(devices::role.ne(DeviceRole::Untrusted))
			.order(devices::created_at.desc())
			.limit(limit)
			.offset(offset)
			.load(db)
			.await
			.map_err(AppError::from)?;

		let device_ids: Vec<Uuid> = trusted_devices.iter().map(|d| d.id).collect();

		let device_keys: Vec<DeviceKey> = device_keys::table
			.select(DeviceKey::as_select())
			.filter(device_keys::device_id.eq_any(&device_ids))
			.filter(device_keys::is_active.eq(true))
			.order(device_keys::created_at.asc())
			.load(db)
			.await
			.map_err(AppError::from)?;

		let latest_connections =
			DeviceConnection::get_latest_from_device_ids(db, device_ids.iter().copied()).await?;

		let mut keys_by_device: HashMap<Uuid, Vec<DeviceKey>> = HashMap::new();
		for key in device_keys {
			keys_by_device.entry(key.device_id).or_default().push(key);
		}

		let mut connections_by_device: HashMap<Uuid, DeviceConnection> = HashMap::new();
		for connection in latest_connections {
			connections_by_device.insert(connection.device_id, connection);
		}

		let result = trusted_devices
			.into_iter()
			.map(|device| DeviceWithInfo {
				keys: keys_by_device.remove(&device.id).unwrap_or_default(),
				latest_connection: connections_by_device.remove(&device.id),
				device,
			})
			.collect();

		Ok(result)
	}

	/// Count untrusted devices.
	pub async fn count_untrusted(db: &mut AsyncPgConnection) -> Result<i64> {
		use crate::schema::devices;
		use diesel::dsl::count_star;

		devices::table
			.filter(devices::role.eq(DeviceRole::Untrusted))
			.select(count_star())
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Count trusted devices.
	pub async fn count_trusted(db: &mut AsyncPgConnection) -> Result<i64> {
		use crate::schema::devices;
		use diesel::dsl::count_star;

		devices::table
			.filter(devices::role.ne(DeviceRole::Untrusted))
			.select(count_star())
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Trust a device by setting its role.
	pub async fn trust(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
		new_role: DeviceRole,
	) -> Result<()> {
		use crate::schema::devices::dsl;

		diesel::update(dsl::devices.filter(dsl::id.eq(device_id)))
			.set(dsl::role.eq(new_role))
			.execute(db)
			.await
			.map_err(AppError::from)?;

		Ok(())
	}

	/// Untrust a device by setting its role to Untrusted.
	pub async fn untrust(db: &mut AsyncPgConnection, device_id: Uuid) -> Result<()> {
		Self::trust(db, device_id, DeviceRole::Untrusted).await
	}

	/// Search devices by key data (supports partial matches).
	pub async fn search_by_key(
		db: &mut AsyncPgConnection,
		query: &str,
	) -> Result<Vec<DeviceWithInfo>> {
		use crate::schema::{device_keys, devices};
		use diesel::sql_query;
		use diesel::sql_types::{Binary, Bool, Uuid as SqlUuid};

		// Try different search strategies
		let mut matching_device_ids: Vec<Uuid> = Vec::new();

		#[derive(QueryableByName)]
		struct MatchingDevice {
			#[diesel(sql_type = SqlUuid)]
			device_id: Uuid,
		}

		// Strategy 1: Try hex decode and binary search
		if let Ok(hex_bytes) = hex::decode(query.replace([' ', ':'], "")) {
			let hex_matches: Vec<Uuid> = sql_query(
				"SELECT DISTINCT device_id FROM device_keys
				 WHERE is_active = $1 AND position($2 in key_data) > 0",
			)
			.bind::<Bool, _>(true)
			.bind::<Binary, _>(&hex_bytes)
			.load::<MatchingDevice>(db)
			.await
			.map_err(AppError::from)?
			.into_iter()
			.map(|m| m.device_id)
			.collect();
			matching_device_ids.extend(hex_matches);
		}

		// Strategy 2: For PEM format, extract base64 and decode
		// Handle both newline-separated and space-separated PEM (from text input fields)
		if query.contains("-----BEGIN") && query.contains("-----END") {
			// Extract everything between BEGIN and END markers
			let begin_marker = "-----BEGIN";
			let end_marker = "-----END";

			if let Some(begin_pos) = query.find(begin_marker)
				&& let Some(end_pos) = query.find(end_marker)
			{
				// Get the content between the markers
				// Find the end of the BEGIN header line
				let begin_header_end = if let Some(newline_pos) = query[begin_pos..].find('\n') {
					begin_pos + newline_pos + 1
				} else {
					// For space-separated PEM, find the end of the header
					// Look for "-----" after the key type (e.g., "PUBLIC KEY-----")
					let after_begin = begin_pos + begin_marker.len(); // Skip "-----BEGIN"
					if let Some(end_marker_pos) = query[after_begin..].find("-----") {
						let header_end = after_begin + end_marker_pos + 5; // +5 for "-----"
						// Find the first space after the complete header
						if let Some(space_pos) = query[header_end..].find(' ') {
							header_end + space_pos + 1
						} else {
							header_end
						}
					} else {
						begin_pos
					}
				};
				let content_start = begin_header_end;

				let pem_content = &query[content_start..end_pos];

				// Check if this is malformed PEM with indented base64 lines
				// Only reject multi-line PEM with indented content, not space-separated single-line PEM
				let lines: Vec<&str> = pem_content.lines().collect();
				let has_indented_lines = lines.len() > 1
					&& lines.iter().any(|line| {
						let trimmed = line.trim();
						!trimmed.is_empty()
							&& !trimmed.starts_with("-----")
							&& line.starts_with([' ', '\t'])
					});

				if !has_indented_lines {
					// Remove any remaining header/footer fragments and whitespace
					let base64_part = pem_content
						.split_whitespace()
						.filter(|part| !part.starts_with("-----") && !part.is_empty())
						.collect::<Vec<_>>()
						.join("");

					if let Ok(decoded) = base64::prelude::BASE64_STANDARD.decode(base64_part) {
						let pem_matches: Vec<Uuid> = sql_query(
							"SELECT DISTINCT device_id FROM device_keys
								 WHERE is_active = $1 AND position($2 in key_data) > 0",
						)
						.bind::<Bool, _>(true)
						.bind::<Binary, _>(&decoded)
						.load::<MatchingDevice>(db)
						.await
						.map_err(AppError::from)?
						.into_iter()
						.map(|m| m.device_id)
						.collect();
						matching_device_ids.extend(pem_matches);
					}
				}
			}
		}

		// Strategy 3: Base64 string search by encoding PostgreSQL binary data
		// PostgreSQL's encode() adds line breaks every 76 chars, so we need to remove them
		if query.len() > 3
			&& query
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
		{
			let base64_matches: Vec<Uuid> = sql_query(
				"SELECT DISTINCT device_id FROM device_keys
				 WHERE is_active = $1 AND replace(encode(key_data, 'base64'), E'\\n', '') LIKE '%' || $2 || '%'",
			)
			.bind::<Bool, _>(true)
			.bind::<diesel::sql_types::Text, _>(query)
			.load::<MatchingDevice>(db)
			.await
			.map_err(AppError::from)?
			.into_iter()
			.map(|m| m.device_id)
			.collect();
			matching_device_ids.extend(base64_matches);
		}

		// Strategy 4: Raw byte search as fallback
		if matching_device_ids.is_empty() {
			let raw_matches: Vec<Uuid> = sql_query(
				"SELECT DISTINCT device_id FROM device_keys
				 WHERE is_active = $1 AND position($2 in key_data) > 0",
			)
			.bind::<Bool, _>(true)
			.bind::<Binary, _>(query.as_bytes())
			.load::<MatchingDevice>(db)
			.await
			.map_err(AppError::from)?
			.into_iter()
			.map(|m| m.device_id)
			.collect();
			matching_device_ids.extend(raw_matches);
		}

		// Remove duplicates
		matching_device_ids.sort();
		matching_device_ids.dedup();

		if matching_device_ids.is_empty() {
			return Ok(vec![]);
		}

		// Get the matching devices
		let matching_devices: Vec<Self> = devices::table
			.select(Self::as_select())
			.filter(devices::id.eq_any(&matching_device_ids))
			.load(db)
			.await
			.map_err(AppError::from)?;

		let device_ids: Vec<Uuid> = matching_devices.iter().map(|d| d.id).collect();

		// Get all keys for matching devices
		let matching_keys: Vec<DeviceKey> = device_keys::table
			.select(DeviceKey::as_select())
			.filter(device_keys::device_id.eq_any(&device_ids))
			.filter(device_keys::is_active.eq(true))
			.load(db)
			.await
			.map_err(AppError::from)?;

		// Get latest connections
		let latest_connections =
			DeviceConnection::get_latest_from_device_ids(db, device_ids.iter().copied()).await?;

		// Group data
		let mut keys_by_device: HashMap<Uuid, Vec<DeviceKey>> = HashMap::new();
		for key in matching_keys {
			keys_by_device.entry(key.device_id).or_default().push(key);
		}

		let mut connections_by_device: HashMap<Uuid, DeviceConnection> = HashMap::new();
		for connection in latest_connections {
			connections_by_device.insert(connection.device_id, connection);
		}

		let result = matching_devices
			.into_iter()
			.map(|device| DeviceWithInfo {
				keys: keys_by_device.remove(&device.id).unwrap_or_default(),
				latest_connection: connections_by_device.remove(&device.id),
				device,
			})
			.collect();

		Ok(result)
	}
}

impl DeviceKey {
	pub async fn create(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
		key: Vec<u8>,
		name: Option<String>,
	) -> Result<Self> {
		use crate::schema::device_keys::dsl;

		diesel::insert_into(dsl::device_keys)
			.values(&(
				dsl::device_id.eq(device_id),
				dsl::key_data.eq(key),
				dsl::name.eq(name),
				dsl::is_active.eq(true),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn find_by_device(db: &mut AsyncPgConnection, device_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::device_keys::dsl;

		dsl::device_keys
			.select(Self::as_select())
			.filter(dsl::device_id.eq(device_id))
			.filter(dsl::is_active.eq(true))
			.order(dsl::created_at.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn deactivate(db: &mut AsyncPgConnection, key_id: Uuid) -> Result<()> {
		use crate::schema::device_keys::dsl;

		diesel::update(dsl::device_keys.filter(dsl::id.eq(key_id)))
			.set(dsl::is_active.eq(false))
			.execute(db)
			.await
			.map_err(AppError::from)?;

		Ok(())
	}
}

#[derive(Clone, Debug, Insertable)]
#[diesel(table_name = crate::schema::device_connections)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewDeviceConnection {
	pub device_id: Uuid,
	pub ip: ipnet::IpNet,
	pub user_agent: Option<String>,
}

impl NewDeviceConnection {
	pub async fn create(&self, db: &mut AsyncPgConnection) -> Result<DeviceConnection> {
		use crate::schema::device_connections::dsl as dc;

		diesel::insert_into(dc::device_connections)
			.values(self)
			.returning(DeviceConnection::as_select())
			.get_result::<DeviceConnection>(db)
			.await
			.map_err(AppError::from)
	}
}

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::device_connections)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DeviceConnection {
	pub id: Uuid,
	pub created_at: DateTime<Utc>,
	pub device_id: Uuid,
	pub ip: ipnet::IpNet,
	pub user_agent: Option<String>,
}

impl DeviceConnection {
	pub async fn get_latest_from_device_ids(
		db: &mut AsyncPgConnection,
		device_ids: impl Iterator<Item = Uuid>,
	) -> Result<Vec<Self>> {
		use crate::schema::device_connections::dsl as dc;

		let ids: Vec<Uuid> = device_ids.collect();
		dc::device_connections
			.select(Self::as_select())
			.distinct_on(dc::device_id)
			.filter(dc::device_id.eq_any(ids))
			.order((dc::device_id, dc::created_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Get connection history for a specific device.
	pub async fn get_history_for_device(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::device_connections::dsl as dc;

		dc::device_connections
			.select(Self::as_select())
			.filter(dc::device_id.eq(device_id))
			.order(dc::created_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Get paginated connection history for a specific device.
	pub async fn get_history_for_device_paginated(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
		limit: i64,
		offset: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::device_connections::dsl as dc;

		dc::device_connections
			.select(Self::as_select())
			.filter(dc::device_id.eq(device_id))
			.order(dc::created_at.desc())
			.limit(limit)
			.offset(offset)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Get total connection count for a specific device.
	pub async fn get_connection_count_for_device(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
	) -> Result<i64> {
		use crate::schema::device_connections::dsl as dc;

		dc::device_connections
			.filter(dc::device_id.eq(device_id))
			.count()
			.get_result(db)
			.await
			.map_err(AppError::from)
	}
}
