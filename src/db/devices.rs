use chrono::{DateTime, Utc};
use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{AsyncPgConnection, prelude::*};
use uuid::Uuid;

use super::device_role::DeviceRole;
use crate::error::{AppError, Result};

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Device {
	/// The ID of the device.
	pub id: Uuid,

	/// The public key data in PublicKeyInfo form.
	///
	/// This is the RFC 5280, Section 4.1.2.7 form of the public key as contained by X.509
	/// certificates or by RFC 7250 Raw Public Keys.
	///
	/// This contains both the public key and its algorithm, and is extensible to support all types
	/// of keys that TLS or X.509 in general can support.
	pub key_data: Vec<u8>,

	/// The role of the device.
	///
	/// This is used for permission checks.
	pub role: DeviceRole,
}

impl Device {
	pub async fn from_key(db: &mut AsyncPgConnection, key: &[u8]) -> Result<Option<Self>> {
		use crate::schema::devices::*;
		table
			.select(Self::as_select())
			.filter(key_data.eq(key))
			.first(db)
			.await
			.optional()
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub async fn create(db: &mut AsyncPgConnection, key: Vec<u8>) -> Result<Self> {
		use crate::schema::devices::*;
		diesel::insert_into(dsl::devices)
			.values(&[(key_data.eq(key))])
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
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
			.map_err(|err| AppError::Database(err.to_string()))
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
