use std::net::{IpAddr, Ipv6Addr};

use chrono::{DateTime, Utc};
use rocket::{
	Config,
	http::{RawStr, Status},
	mtls::Certificate,
	outcome::{IntoOutcome, try_outcome},
	request::{self, Outcome},
	serde::{Deserialize, Serialize},
};
use rocket_db_pools::{
	Connection,
	diesel::{AsyncPgConnection, prelude::*},
};
use uuid::Uuid;

use super::device_role::DeviceRole;
use crate::{
	db::Db,
	error::{AppError, Result},
};

macro_rules! device_role_struct {
	($name:ident, $allowed_role:expr) => {
		#[derive(Clone, Debug)]
		pub struct $name(#[allow(dead_code)] pub Device);

		#[rocket::async_trait]
		impl<'r> request::FromRequest<'r> for $name {
			type Error = AppError;

			async fn from_request(req: &'r request::Request<'_>) -> Outcome<Self, Self::Error> {
				let device = try_outcome!(req.guard::<Device>().await);
				if device.role == DeviceRole::Admin || device.role == $allowed_role {
					Outcome::Success(Self(device))
				} else {
					Outcome::Error((
						Status::Forbidden,
						AppError::custom(format!(
							"device is not a {}",
							stringify!($name).to_lowercase()
						)),
					))
				}
			}
		}
	};
}

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

#[rocket::async_trait]
impl<'r> request::FromRequest<'r> for Device {
	type Error = AppError;

	async fn from_request(req: &'r request::Request<'_>) -> Outcome<Self, Self::Error> {
		use x509_parser::prelude::*;

		let mut db = match req.guard::<Connection<Db>>().await {
			Outcome::Success(db) => db,
			Outcome::Forward(f) => return Outcome::Forward(f),
			Outcome::Error((s, e)) => {
				return Outcome::Error((
					s,
					e.map_or(AppError::custom("unknown request db guard error"), |e| {
						AppError::Database(format!("{e:?}"))
					}),
				));
			}
		};

		let key = match req.guard::<Certificate>().await {
			Outcome::Success(cert) => cert.subject_pki.raw.to_vec(),
			Outcome::Error((s, e)) => {
				// certificate presented, but fails validation
				return Outcome::Error((s, AppError::custom(e)));
			}
			Outcome::Forward(_) => {
				// certificate not presented

				let Outcome::Success(config) = req.guard::<&Config>().await else {
					panic!("Config guard always returns successfully")
				};

				if config
					.tls
					.as_ref()
					.is_some_and(|tls| tls.mutual().is_some())
				{
					// rocket is handling mTLS, so refuse to process mtls-certificate header
					return Outcome::Forward(Status::Forbidden);
				}

				let pem = try_outcome!(
					req.headers()
						.get_one("mtls-certificate")
						.or_else(|| req.headers().get_one("ssl-client-cert"))
						.ok_or_else(|| AppError::custom("missing mtls-certificate header"))
						.and_then(|s| RawStr::new(s).url_decode().map_err(AppError::custom))
						.or_error(Status::BadRequest)
				);

				let (_, der) = try_outcome!(
					parse_x509_pem(pem.as_bytes())
						.map_err(AppError::custom)
						.or_error(Status::BadRequest)
				);
				let (_, cert) = try_outcome!(
					parse_x509_certificate(&der.contents)
						.map_err(AppError::custom)
						.or_error(Status::BadRequest)
				);

				cert.tbs_certificate.subject_pki.raw.to_vec()
			}
		};

		let device = if let Some(existing) = try_outcome!(
			Self::from_key(&mut db, &key)
				.await
				.or_error(Status::InternalServerError)
		) {
			existing
		} else {
			try_outcome!(
				Device::create(&mut db, key)
					.await
					.or_error(Status::InternalServerError)
			)
		};

		let _ = try_outcome!(
			NewDeviceConnection {
				device_id: device.id,
				ip: req
					.client_ip()
					.unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED))
					.into(),
				user_agent: req.headers().get_one("user-agent").map(|s| s.to_string()),
			}
			.create(&mut db)
			.await
			.or_error(Status::InternalServerError)
		);

		Outcome::Success(device)
	}
}

device_role_struct!(AdminDevice, DeviceRole::Admin);
device_role_struct!(ServerDevice, DeviceRole::Server);
device_role_struct!(ReleaserDevice, DeviceRole::Releaser);

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
