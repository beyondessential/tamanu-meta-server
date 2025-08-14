use std::{
	net::{IpAddr, Ipv6Addr, SocketAddr},
	str::FromStr as _,
};

use axum::{
	RequestPartsExt,
	extract::{ConnectInfo, FromRef},
};

use crate::{
	db::{
		device_role::DeviceRole,
		devices::{Device, NewDeviceConnection},
	},
	error::AppError,
	state,
};

macro_rules! device_role_struct {
	($name:ident, $allowed_role:expr) => {
		#[derive(Clone, Debug)]
		pub struct $name(#[allow(dead_code)] pub Device);

		impl<S> axum::extract::FromRequestParts<S> for $name
		where
			state::Db: FromRef<S>,
			S: Send + Sync,
		{
			type Rejection = AppError;

			async fn from_request_parts(
				parts: &mut axum::http::request::Parts,
				state: &S,
			) -> Result<Self, Self::Rejection> {
				let device = Device::from_request_parts(parts, state).await?;
				if device.role == DeviceRole::Admin || device.role == $allowed_role {
					Ok(Self(device))
				} else {
					Err(AppError::custom(format!(
						"device is not a {}",
						stringify!($name).to_lowercase()
					)))
				}
			}
		}
	};
}

device_role_struct!(AdminDevice, DeviceRole::Admin);
device_role_struct!(ServerDevice, DeviceRole::Server);
device_role_struct!(ReleaserDevice, DeviceRole::Releaser);

impl<S> axum::extract::FromRequestParts<S> for Device
where
	state::Db: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(
		parts: &mut axum::http::request::Parts,
		state: &S,
	) -> Result<Self, Self::Rejection> {
		use axum::http::header::USER_AGENT;
		use x509_parser::prelude::*;

		let mut db = state::Db::from_ref(state).get().await?;

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

		let device = if let Some(existing) = Self::from_key(&mut db, &key).await? {
			existing
		} else {
			Device::create(&mut db, key).await?
		};

		let user_agent = parts
			.headers
			.get(USER_AGENT)
			.and_then(|s| s.to_str().ok())
			.map(|s| s.to_owned());

		let client_ip: Option<ConnectInfo<SocketAddr>> = parts.extract().await.ok();
		let ip = parts
			.headers
			.get("x-forwarded-for")
			.and_then(|s| s.to_str().ok())
			.and_then(|s| IpAddr::from_str(s).ok())
			.or_else(|| client_ip.map(|c| c.ip()))
			.unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED));

		NewDeviceConnection {
			device_id: device.id,
			ip: ip.into(),
			user_agent,
		}
		.create(&mut db)
		.await?;

		Ok(device)
	}
}
