use std::net::{IpAddr, Ipv6Addr};

use rocket::{
	http::Status,
	outcome::{try_outcome, IntoOutcome},
	request::{self, Outcome},
	serde::{Deserialize, Serialize},
};
use rocket_db_pools::{
	diesel::{prelude::*, AsyncPgConnection},
	Connection,
};
use uuid::Uuid;

use super::device_role::DeviceRole;
use crate::db::Db;

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Device {
	pub id: Uuid,
	pub key_checksum: Vec<u8>,
	pub role: DeviceRole,
}

impl Device {
	pub async fn from_key(
		db: &mut AsyncPgConnection,
		key_sha256: &[u8],
	) -> Result<Option<Self>, AppError> {
		use crate::schema::devices::*;
		table
			.select(Self::as_select())
			.filter(key_checksum.eq(key_sha256))
			.first(db)
			.await
			.optional()
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub async fn create(db: &mut AsyncPgConnection, key_sha256: Vec<u8>) -> Result<Self, AppError> {
		use crate::schema::devices::*;
		diesel::insert_into(dsl::devices)
			.values(&[(key_checksum.eq(key_sha256))])
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}
}

#[rocket::async_trait]
impl<'r> request::FromRequest<'r> for Device {
	type Error = AppError;

	async fn from_request(req: &'r request::Request<'_>) -> Outcome<Self, Self::Error> {
		let mut db = match req.guard::<Connection<Db>>().await {
			Outcome::Success(db) => db,
			Outcome::Forward(f) => return Outcome::Forward(f),
			Outcome::Error((s, e)) => {
				return Outcome::Error((
					s,
					e.map_or(AppError::custom("unknown request db guard error"), |e| {
						AppError::Database(format!("{e:?}"))
					}),
				))
			}
		};

		let headers = req.headers();

		let pkeysum = try_outcome!(headers
			.get_one("x-mtls-public-key-sha256")
			.ok_or_else(|| AppError::custom("missing x-mtls-public-key-sha256 header"))
			.and_then(|s| hex::decode(s).map_err(AppError::custom))
			.or_error(Status::BadRequest));

		let device = if let Some(existing) = try_outcome!(Self::from_key(&mut db, &pkeysum)
			.await
			.or_error(Status::InternalServerError))
		{
			existing
		} else {
			info!("recording new device: {pkeysum:x?}");
			try_outcome!(Device::create(&mut db, pkeysum)
				.await
				.or_error(Status::InternalServerError))
		};

		try_outcome!(DeviceConnection {
			device_id: device.id,
			ip: req
				.client_ip()
				.unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED))
				.into(),
			user_agent: req.headers().get_one("user-agent").map(|s| s.to_string()),
		}
		.create(&mut db)
		.await
		.or_error(Status::InternalServerError));

		Outcome::Success(device)
	}
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
	#[error("{0}")]
	Custom(String),

	// it's practically impossible to wrangle rocket's actual db error here, so string it
	#[error("database: {0}")]
	Database(String),
}

impl AppError {
	pub fn custom(err: impl ToString) -> Self {
		Self::Custom(err.to_string())
	}
}

#[derive(Clone, Debug)]
pub struct AdminDevice(#[allow(dead_code)] pub Device);

#[rocket::async_trait]
impl<'r> request::FromRequest<'r> for AdminDevice {
	type Error = AppError;

	async fn from_request(req: &'r request::Request<'_>) -> Outcome<Self, Self::Error> {
		let device = try_outcome!(req.guard::<Device>().await);
		if device.role == DeviceRole::Admin {
			Outcome::Success(Self(device))
		} else {
			Outcome::Error((
				Status::Forbidden,
				AppError::custom("device is not an admin"),
			))
		}
	}
}

#[derive(Clone, Debug)]
pub struct ServerDevice(#[allow(dead_code)] pub Device);

#[rocket::async_trait]
impl<'r> request::FromRequest<'r> for ServerDevice {
	type Error = AppError;

	async fn from_request(req: &'r request::Request<'_>) -> Outcome<Self, Self::Error> {
		let device = try_outcome!(req.guard::<Device>().await);
		if device.role == DeviceRole::Admin || device.role == DeviceRole::Server {
			Outcome::Success(Self(device))
		} else {
			Outcome::Error((
				Status::Forbidden,
				AppError::custom("device is not a server"),
			))
		}
	}
}

#[derive(Clone, Debug, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::device_connections)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DeviceConnection {
	pub device_id: Uuid,
	pub ip: ipnet::IpNet,
	pub user_agent: Option<String>,
}

impl DeviceConnection {
	pub async fn create(&self, db: &mut AsyncPgConnection) -> Result<Self, AppError> {
		diesel::insert_into(crate::schema::device_connections::dsl::device_connections)
			.values(self)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}
}
