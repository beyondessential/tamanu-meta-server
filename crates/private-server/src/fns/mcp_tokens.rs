//! Admin management of the bearer tokens gating the public MCP mount.
//!
//! Spec: `.workhorse/specs/private-server/mcp.md` (id `MCP`), "Access tokens".
//!
//! `mint` is the only place the token plaintext ever leaves the system; the
//! row views carry metadata only.

use axum::Json;
use axum::extract::State;
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::Uuid;
use database::mcp_tokens::McpToken;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(mint))
		.routes(routes!(revoke))
}

/// A token row for the operator UI: everything but the secret (only a hash of
/// which exists server-side anyway).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpTokenView {
	pub id: Uuid,
	pub name: String,
	pub created_by: String,
	pub created_at: Timestamp,
	pub expires_at: Timestamp,
	pub revoked_at: Option<Timestamp>,
	pub last_used_at: Option<Timestamp>,
}

impl From<McpToken> for McpTokenView {
	fn from(t: McpToken) -> Self {
		Self {
			id: t.id,
			name: t.name,
			created_by: t.created_by,
			created_at: t.created_at,
			expires_at: t.expires_at,
			revoked_at: t.revoked_at,
			last_used_at: t.last_used_at,
		}
	}
}

#[utoipa::path(
	post,
	path = "/list",
	operation_id = "mcp_tokens_list",
	tag = "mcp_tokens",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, description = "All MCP access tokens, newest first, revoked included.", body = Vec<McpTokenView>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
) -> Result<Json<Vec<McpTokenView>>> {
	let mut conn = state.db.get().await?;
	let tokens = McpToken::list(&mut conn).await?;
	Ok(Json(tokens.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MintArgs {
	/// Operator-chosen label, e.g. which agent will hold this token.
	pub name: String,
}

/// The one and only exposure of the token plaintext.
#[derive(Debug, Serialize, ToSchema)]
pub struct MintedToken {
	pub token: McpTokenView,
	/// The bearer token itself. Shown once; never retrievable again.
	pub secret: String,
}

#[utoipa::path(
	post,
	path = "/mint",
	operation_id = "mcp_tokens_mint",
	tag = "mcp_tokens",
	security(("tailscale-admin" = [])),
	request_body = MintArgs,
	responses(
		(status = 200, description = "Freshly minted token; the secret in this response is shown once.", body = MintedToken),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn mint(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<MintArgs>,
) -> Result<Json<MintedToken>> {
	let name = args.name.trim();
	if name.is_empty() {
		return Err(commons_errors::AppError::BadRequest(
			"token name cannot be empty".into(),
		));
	}
	let mut conn = state.db.get().await?;
	let (token, secret) = McpToken::mint(&mut conn, name, &admin.login).await?;
	Ok(Json(MintedToken {
		token: token.into(),
		secret,
	}))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RevokeArgs {
	pub id: Uuid,
}

#[utoipa::path(
	post,
	path = "/revoke",
	operation_id = "mcp_tokens_revoke",
	tag = "mcp_tokens",
	security(("tailscale-admin" = [])),
	request_body = RevokeArgs,
	responses(
		(status = 200, description = "Token revoked (idempotent)."),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn revoke(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<RevokeArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	McpToken::revoke(&mut conn, args.id).await?;
	Ok(Json(()))
}
