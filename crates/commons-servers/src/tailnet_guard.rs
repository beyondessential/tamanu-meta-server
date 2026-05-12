//! Reject tagged-device callers on routes that aren't `/public/...`.
//!
//! The dual-auth device extractor in [`crate::device_auth`] welcomes
//! tagged tailnet devices on the public-server router that's nested
//! under private-server's `/public` mount. Every other surface
//! (`/api/*`, the SPA fallback, Swagger UI) is for human admins and
//! internal callers — a tagged device hitting one of those is either
//! misconfigured or probing, and should get a quick 403 rather than a
//! 200 of static HTML or a confusing-to-debug 401 from a downstream
//! extractor.
//!
//! Wired as an axum middleware layer applied only to the non-`/public`
//! subtree in `private-server::routes()`.

use axum::{
	RequestPartsExt as _,
	extract::Request,
	middleware::Next,
	response::{IntoResponse, Response},
};
use axum_client_ip::ClientIp;
use commons_errors::AppError;

use crate::tailnet_directory::is_tailnet_ip;

/// Reject the request when it looks like it's coming from a tagged
/// tailnet device:
///
/// 1. No `Tailscale-User-Login` header. Tailscale Serve / the K8s
///    Operator's ingress proxy injects identity headers for logged-in
///    *humans*; tagged devices get nothing.
/// 2. `ClientIp` (resolved by axum-client-ip from the configured
///    forwarded-headers source) is in the Tailscale CGNAT v4 or ULA v6
///    ranges. Either prefix is non-routable outside the tailnet, so a
///    non-tailnet caller can't end up with one as their resolved
///    client IP.
///
/// `/public/...` is exempt — that's the only surface that welcomes
/// tagged-device callers, via the dual-auth device extractor.
pub async fn reject_tagged_devices(req: Request, next: Next) -> Response {
	if req.uri().path().starts_with("/public/") {
		return next.run(req).await;
	}
	if req.headers().contains_key("tailscale-user-login") {
		return next.run(req).await;
	}

	let (mut parts, body) = req.into_parts();
	let client_ip = parts.extract::<ClientIp>().await.ok().map(|c| c.0);
	let req = Request::from_parts(parts, body);

	if let Some(ip) = client_ip
		&& is_tailnet_ip(ip)
	{
		return AppError::TaggedDeviceNotAllowed.into_response();
	}
	next.run(req).await
}
