//! Tailnet path of the device-auth extractor. Reads the calling node's
//! Tailscale CGNAT v4 or ULA v6 address from the request's `ClientIp`,
//! resolves it via the [`TailnetDirectory`], and keys into
//! `devices.tailscale_node_id`. An unknown node is not authenticated: devices
//! are only created when an operator provisions or attaches them.

use axum::RequestPartsExt as _;
use axum_client_ip::ClientIp;
use commons_errors::{AppError, Result};
use database::devices::Device;
use diesel_async::AsyncPgConnection;
use http::request::Parts;

use crate::tailnet_directory::{TailnetDirectory, is_tailnet_ip};

/// Resolve a request to a [`Device`] via the tailnet directory. Returns
/// `Ok(Some((device, node_id)))` on success, `Ok(None)` if the request's
/// source isn't a tailnet address (so the caller should fall through to
/// the next path), or `Err(_)` on a directory or tag-policy failure.
pub async fn resolve(
	parts: &mut Parts,
	db: &mut AsyncPgConnection,
	directory: &TailnetDirectory,
) -> Result<Option<(Device, String)>> {
	let Ok(ClientIp(ip)) = parts.extract::<ClientIp>().await else {
		return Ok(None);
	};

	if !is_tailnet_ip(ip) {
		return Ok(None);
	}

	let Some(entry) = directory
		.lookup(ip)
		.await
		.map_err(|_| AppError::AuthTailnetDirectoryUnavailable)?
	else {
		// IP is in the tailnet space but the directory doesn't know it.
		// Treat as "not authenticated by this path" rather than an error —
		// might be a freshly-joined node or a stale cache; the mTLS path
		// still gets a shot.
		return Ok(None);
	};

	if let Ok(required) = std::env::var("TAILSCALE_REQUIRED_TAG")
		&& !required.is_empty()
		&& !entry.tags.iter().any(|t| t == &required)
	{
		return Err(AppError::AuthTailnetNodeNotPermitted);
	}

	// A tailnet node is only ever a device once an operator has provisioned or
	// attached it. An unknown node is not auto-created — it simply fails to
	// authenticate (the caller maps `None` to an auth error). This avoids
	// minting inert placeholder rows for every node that touches the tunnel.
	let Some(device) = Device::from_tailscale_node_id(db, &entry.node_id).await? else {
		return Ok(None);
	};

	Ok(Some((device, entry.node_id)))
}
