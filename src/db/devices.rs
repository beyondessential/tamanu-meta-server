use std::net::{IpAddr, Ipv6Addr};

use ed25519_dalek::{SignatureError, VerifyingKey};
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

use super::device_role::DeviceRole;
use crate::db::Db;

#[derive(Clone, Debug, Default, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Device {
	public_key: Vec<u8>,
	pub role: DeviceRole,
}

impl Device {
	pub async fn from_key(
		db: &mut AsyncPgConnection,
		key: [u8; 32],
	) -> Result<Option<Self>, AppError> {
		use crate::schema::devices::*;
		table
			.select(Self::as_select())
			.find(key.as_slice())
			.first(db)
			.await
			.optional()
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub fn new(key: [u8; 32]) -> Self {
		Self {
			public_key: key.to_vec(),
			..Default::default()
		}
	}

	pub async fn create(&self, db: &mut AsyncPgConnection) -> Result<Self, AppError> {
		diesel::insert_into(crate::schema::devices::dsl::devices)
			.values(self)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub fn key(&self) -> Result<VerifyingKey, AppError> {
		let bytes = <[u8; 32]>::try_from(self.public_key.as_slice())
			.map_err(|_| AppError::custom("devices.public_key is not 32-bytes long"))?;
		let key = VerifyingKey::from_bytes(&bytes)?;
		if key.is_weak() {
			Err(AppError::custom("weak keys are not allowed"))
		} else {
			Ok(key)
		}
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

		let header = try_outcome!(req
			.headers()
			.get_one("x-mtls-public-key")
			.ok_or_else(|| AppError::custom("missing x-mtls-public-key header"))
			.or_error(Status::InternalServerError));
		let bytes = try_outcome!(hex::decode(header)
			.map_err(AppError::custom)
			.or_error(Status::InternalServerError));
		let key = try_outcome!(<[u8; 32]>::try_from(bytes.as_slice())
			.map_err(|_| AppError::custom("public key is not 32-bytes long"))
			.or_error(Status::InternalServerError));

		let device = if let Some(existing) = try_outcome!(Self::from_key(&mut db, key)
			.await
			.or_error(Status::InternalServerError))
		{
			existing
		} else {
			info!("recording new device: {header}");
			let device = Self::new(key);
			try_outcome!(device
				.create(&mut db)
				.await
				.or_error(Status::InternalServerError));
			device
		};

		try_outcome!(DeviceConnection {
			device: device.public_key.clone(),
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

	#[error("crypto: {0}")]
	Ed25519(#[from] SignatureError),
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
	pub device: Vec<u8>,
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
