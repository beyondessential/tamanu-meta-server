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

	/// List all untrusted devices with their keys and latest connection info.
	pub async fn list_untrusted_with_info(
		db: &mut AsyncPgConnection,
	) -> Result<Vec<DeviceWithInfo>> {
		use crate::schema::{device_keys, devices};

		let untrusted_devices: Vec<Self> = devices::table
			.select(Self::as_select())
			.filter(devices::role.eq(DeviceRole::Untrusted))
			.order(devices::created_at.desc())
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

	/// Trust a device by updating its role.
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

	/// Search devices by key data (supports partial matches).
	pub async fn search_by_key(
		db: &mut AsyncPgConnection,
		query: &str,
	) -> Result<Vec<DeviceWithInfo>> {
		use crate::schema::{device_keys, devices};
		use diesel::sql_query;
		use diesel::sql_types::{Binary, Bool, Uuid as SqlUuid};

		// Try to decode hex query (with or without colons/spaces)
		let search_bytes = if let Ok(hex_bytes) = hex::decode(query.replace([' ', ':'], "")) {
			hex_bytes
		} else {
			// For PEM format, try to extract the base64 part and decode it
			if query.contains("-----BEGIN") && query.contains("-----END") {
				let base64_part = query
					.lines()
					.filter(|line| !line.starts_with("-----"))
					.collect::<Vec<_>>()
					.join("");

				if let Ok(decoded) = base64::prelude::BASE64_STANDARD.decode(base64_part) {
					decoded
				} else {
					query.as_bytes().to_vec()
				}
			} else {
				query.as_bytes().to_vec()
			}
		};

		#[derive(QueryableByName)]
		struct MatchingDevice {
			#[diesel(sql_type = SqlUuid)]
			device_id: Uuid,
		}

		// Use PostgreSQL's position function to search for the byte sequence
		let matching_device_ids: Vec<Uuid> = sql_query(
			"SELECT DISTINCT device_id FROM device_keys
			 WHERE is_active = $1 AND position($2 in key_data) > 0",
		)
		.bind::<Bool, _>(true)
		.bind::<Binary, _>(&search_bytes)
		.load::<MatchingDevice>(db)
		.await
		.map_err(AppError::from)?
		.into_iter()
		.map(|m| m.device_id)
		.collect();

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
