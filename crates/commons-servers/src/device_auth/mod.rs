//! Device authentication for the public-server and private-server's
//! `/public/...` mount.
//!
//! Two paths share the extractor:
//!
//! - **mTLS** ([`mtls`]): the existing cert-in-header flow. Devices
//!   present a client certificate; the public key derived from it keys
//!   into `device_keys.key_data`. First-contact auto-creates the
//!   `Device` row.
//! - **Tailnet** ([`tailnet`]): the new path. Available only on the
//!   private-server (its `AppState` carries a populated
//!   [`TailnetDirectory`]; public-server's yields `None` so this path is
//!   short-circuited). Reads the calling node's CGNAT v4 or ULA v6
//!   address from `X-Forwarded-For` (via `axum-client-ip`'s `ClientIp`),
//!   resolves it to a node identity through the Tailscale control plane
//!   API, then keys into `devices.tailscale_node_id`. First-contact
//!   auto-creates a `Device` row with role `Untrusted`.
//!
//! Order: mTLS is tried first, biased to preserve the current behaviour
//! for any device that happens to present both. The combined extractor
//! emits `AuthMissingCertificate` only if both paths yielded `None`.

use std::net::{IpAddr, Ipv6Addr};

use axum::{RequestPartsExt as _, extract::FromRef};
use axum_client_ip::ClientIp;
use commons_errors::AppError;
use commons_types::device::DeviceRole;
use database::{
	Db,
	devices::{Device, NewDeviceConnection},
};

use crate::tailnet_directory::TailnetDirectory;

pub mod mtls;
pub mod tailnet;

/// Which path produced this [`AuthDevice`]. Carried alongside the device
/// for connection logging and future audit; handlers don't need to
/// inspect it.
#[derive(Clone, Debug)]
pub enum AuthMethod {
	Mtls,
	Tailnet { node_id: String },
}

#[derive(Debug, Clone)]
pub struct AuthDevice(pub Device, pub AuthMethod);

macro_rules! device_role_struct {
	($name:ident, $allowed_role:expr) => {
		#[derive(Clone, Debug)]
		pub struct $name(#[allow(dead_code)] pub AuthDevice);

		impl<S> axum::extract::FromRequestParts<S> for $name
		where
			Db: FromRef<S>,
			Option<TailnetDirectory>: FromRef<S>,
			S: Send + Sync,
		{
			type Rejection = AppError;

			async fn from_request_parts(
				parts: &mut axum::http::request::Parts,
				state: &S,
			) -> Result<Self, Self::Rejection> {
				let auth = AuthDevice::from_request_parts(parts, state).await?;
				if auth.0.role == DeviceRole::Admin || auth.0.role == $allowed_role {
					Ok(Self(auth))
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
	Option<TailnetDirectory>: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(
		parts: &mut axum::http::request::Parts,
		state: &S,
	) -> Result<Self, Self::Rejection> {
		let mut db = Db::from_ref(state).get().await?;

		// Bias to mTLS so any device presenting both falls into the
		// existing path; the tailnet path picks up only the cert-less
		// callers.
		let resolved = if let Some(device) = mtls::resolve(parts, &mut db).await? {
			Some((device, AuthMethod::Mtls))
		} else if let Some(directory) = Option::<TailnetDirectory>::from_ref(state) {
			tailnet::resolve(parts, &mut db, &directory)
				.await?
				.map(|(device, node_id)| (device, AuthMethod::Tailnet { node_id }))
		} else {
			None
		};

		let (device, method) = resolved.ok_or(AppError::AuthMissingCertificate)?;

		let user_agent = parts
			.headers
			.get(axum::http::header::USER_AGENT)
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

		Ok(Self(device, method))
	}
}
