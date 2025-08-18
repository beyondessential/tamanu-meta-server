use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::device_role::DeviceRole;
use crate::error::{AppError, Result};

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Device {
	/// The ID of the device.
	pub id: Uuid,

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
}
