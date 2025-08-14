use std::net::{IpAddr, Ipv6Addr};

use rocket::{
	http::{RawStr, Status},
	outcome::{IntoOutcome, try_outcome},
	request::{self, Outcome},
};
use rocket_db_pools::Connection;

use crate::{
	db::{
		Db,
		device_role::DeviceRole,
		devices::{Device, NewDeviceConnection},
	},
	error::AppError,
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
						AppError::custom(format!("{e:?}"))
					}),
				));
			}
		};

		let key = {
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

impl<S> axum::extract::FromRequestParts<S> for Device
where
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(
		parts: &mut axum::http::request::Parts,
		state: &S,
	) -> Result<Self, Self::Rejection> {
		use axum::{
			Router,
			extract::FromRequestParts,
			http::{
				StatusCode,
				header::{HeaderValue, USER_AGENT},
				request::Parts,
			},
			routing::get,
		};
		use x509_parser::prelude::*;

		let key = {
			let pem = parts
				.headers
				.get("mtls-certificate")
				.or_else(|| parts.headers.get("ssl-client-cert"))
				.ok_or_else(|| AppError::custom("missing mtls-certificate header"))
				.and_then(|s| {
					percent_encoding::percent_decode(s.as_bytes())
						.decode_utf8()
						.map_err(AppError::custom)
				})?;

			let (_, der) = parse_x509_pem(pem.as_bytes()).map_err(AppError::custom)?;
			let (_, cert) = parse_x509_certificate(&der.contents).map_err(AppError::custom)?;

			cert.tbs_certificate.subject_pki.raw.to_vec()
		};

		todo!()
	}
}
