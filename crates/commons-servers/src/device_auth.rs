use std::net::{IpAddr, Ipv6Addr};

use axum::{RequestPartsExt as _, extract::FromRef};
use axum_client_ip::ClientIp;
use commons_errors::AppError;
use database::{
	Db,
	device_role::DeviceRole,
	devices::{Device, NewDeviceConnection},
};

#[derive(Debug, Clone)]
pub struct AuthDevice(pub Device);

macro_rules! device_role_struct {
	($name:ident, $allowed_role:expr) => {
		#[derive(Clone, Debug)]
		pub struct $name(#[allow(dead_code)] pub AuthDevice);

		impl<S> axum::extract::FromRequestParts<S> for $name
		where
			Db: FromRef<S>,
			S: Send + Sync,
		{
			type Rejection = AppError;

			async fn from_request_parts(
				parts: &mut axum::http::request::Parts,
				state: &S,
			) -> Result<Self, Self::Rejection> {
				let device = AuthDevice::from_request_parts(parts, state).await?;
				if device.0.role == DeviceRole::Admin || device.0.role == $allowed_role {
					Ok(Self(device))
				} else {
					Err(AppError::AuthInsufficientPermissions {
						required: format!("{} or admin", stringify!($name).to_lowercase()),
					})
				}
			}
		}
	};
}

device_role_struct!(AdminDevice, DeviceRole::Admin);
device_role_struct!(ServerDevice, DeviceRole::Server);
device_role_struct!(ReleaserDevice, DeviceRole::Releaser);

impl<S> axum::extract::FromRequestParts<S> for AuthDevice
where
	Db: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(
		parts: &mut axum::http::request::Parts,
		state: &S,
	) -> Result<Self, Self::Rejection> {
		use axum::http::header::USER_AGENT;
		use x509_parser::prelude::*;

		let mut db = Db::from_ref(state).get().await?;

		let key = {
			let pem = parts
				.headers
				.get("mtls-certificate")
				.or_else(|| parts.headers.get("ssl-client-cert"))
				.ok_or(AppError::AuthMissingCertificate)
				.and_then(|s| {
					percent_encoding::percent_decode(s.as_bytes())
						.decode_utf8()
						.map_err(|e| {
							AppError::AuthInvalidCertificate(format!(
								"Invalid UTF-8 in certificate: {}",
								e
							))
						})
				})?;

			let (_, der) = parse_x509_pem(pem.as_bytes()).map_err(|e| {
				AppError::AuthInvalidCertificate(format!("Invalid PEM format: {}", e))
			})?;
			let (_, cert) = parse_x509_certificate(&der.contents).map_err(|e| {
				AppError::AuthInvalidCertificate(format!("Invalid X.509 certificate: {}", e))
			})?;

			cert.tbs_certificate.subject_pki.raw.to_vec()
		};

		let device = if let Some(existing) = Device::from_key(&mut db, &key).await? {
			existing
		} else {
			// Create new device for unknown certificates (existing behavior)
			// In production, you may want to disable auto-creation for security
			Device::create(&mut db, key)
				.await
				.map_err(|e| AppError::AuthFailed {
					reason: format!("Failed to create device: {}", e),
				})?
		};

		let user_agent = parts
			.headers
			.get(USER_AGENT)
			.and_then(|s| s.to_str().ok())
			.map(|s| s.to_owned());

		let client_ip: Option<ClientIp> = parts.extract().await.ok();
		let ip = client_ip.map_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED), |c| c.0);

		NewDeviceConnection {
			device_id: device.id,
			ip: ip.into(),
			user_agent,
		}
		.create(&mut db)
		.await?;

		Ok(Self(device))
	}
}
