//! The operator-surface gate for the MCP mount; the interface itself lives in
//! the `canopy-mcp` crate.
//!
//! Spec: `.workhorse/specs/private-server/mcp.md` (id `MCP`).

/// Gate the MCP mount on an authenticated tailnet user (any user, not only
/// admins). Reuses the operator surface's `TailscaleUser` extractor, so the
/// debug-build dev bypass applies in local dev and tests. The caller's login is
/// logged so each query is attributable, and inserted as the request's
/// [`canopy_mcp::McpIdentity`] — any tailnet user may use the write tools.
pub async fn require_tailnet_user(
	req: axum::extract::Request,
	next: axum::middleware::Next,
) -> Result<axum::response::Response, commons_errors::AppError> {
	use axum::extract::FromRequestParts as _;
	use commons_servers::tailscale_auth::TailscaleUser;

	let (mut parts, body) = req.into_parts();
	let user = TailscaleUser::from_request_parts(&mut parts, &()).await?;
	tracing::info!(login = %user.login, "mcp request");
	parts.extensions.insert(canopy_mcp::McpIdentity {
		who: user.login.clone(),
		can_write: true,
	});
	let req = axum::extract::Request::from_parts(parts, body);
	Ok(next.run(req).await)
}
