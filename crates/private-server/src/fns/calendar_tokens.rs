//! Admin management of the tokens gating the planned-upgrades calendar feed.
//!
//! Spec: `.workhorse/specs/private-server/upgrade-plans.md` (id `UPG`), "The
//! calendar feed".
//!
//! `mint` is the only place the feed URL ever leaves the system; the row views
//! carry metadata only.

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::Uuid;
use database::calendar_tokens::CalendarToken;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(mint))
		.routes(routes!(revoke))
}

/// The feed URL for a token, built from the configured public API base URL.
/// `None` when none is configured, in which case the caller has only a path to
/// offer.
fn feed_url(secret: &str) -> Option<String> {
	let base = std::env::var("PUBLIC_URL").ok()?;
	Some(format!(
		"{}/calendar/{secret}/upgrades.ics",
		base.trim_end_matches('/')
	))
}

/// Metadata about a calendar feed. Never includes the token in its URL — that
/// is only ever returned once, at minting time.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CalendarTokenView {
	/// Unique identifier of the feed.
	pub id: Uuid,
	/// Operator-chosen label for who or what subscribes to it.
	pub name: String,
	/// The login of the admin who minted it.
	pub created_by: String,
	/// When it was minted.
	pub created_at: Timestamp,
	/// When it was revoked, or `null` if it has not been.
	pub revoked_at: Option<Timestamp>,
	/// When a calendar client last fetched it, or `null` if none ever has. May
	/// lag the true last fetch by up to a minute.
	pub last_used_at: Option<Timestamp>,
}

impl From<CalendarToken> for CalendarTokenView {
	fn from(t: CalendarToken) -> Self {
		Self {
			id: t.id,
			name: t.name,
			created_by: t.created_by,
			created_at: t.created_at,
			revoked_at: t.revoked_at,
			last_used_at: t.last_used_at,
		}
	}
}

/// List calendar feeds.
///
/// Returns every feed that has ever been minted, newest first, including ones
/// that have since been revoked. Feed URLs are never included, only metadata.
#[utoipa::path(
	post,
	path = "/list",
	operation_id = "calendar_tokens_list",
	tag = "calendar_tokens",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, description = "All calendar feeds, newest first, revoked included.", body = Vec<CalendarTokenView>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
) -> Result<Json<Vec<CalendarTokenView>>> {
	let mut conn = state.db.get().await?;
	let tokens = CalendarToken::list(&mut conn).await?;
	Ok(Json(tokens.into_iter().map(Into::into).collect()))
}

/// Request body for minting a new calendar feed. Named apart from the MCP
/// one because utoipa keys component schemas by short name.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MintCalendarArgs {
	/// Operator-chosen label, e.g. whose calendar this is for. Cannot be empty
	/// or only whitespace.
	pub name: String,
}

/// The result of minting a calendar feed: its metadata plus the one-time URL.
/// This is the only response that will ever include the URL.
#[derive(Debug, Serialize, ToSchema)]
pub struct MintedCalendar {
	/// Metadata about the newly minted feed.
	pub token: CalendarTokenView,
	/// The subscription URL. Shown once; never retrievable again. `null` when
	/// the public API base URL is not configured, in which case `path` is all
	/// there is to go on.
	pub url: Option<String>,
	/// The feed's path on the public API host, for building the URL by hand.
	pub path: String,
}

/// Mint a calendar feed.
///
/// Returns a subscription URL that reads planned upgrades, for pasting into a
/// calendar application. The URL appears only in this response and cannot be
/// retrieved again, so it must be copied out immediately. Anyone holding it can
/// read the feed, which is what lets a calendar service fetch it unattended.
/// Returns 400 if the supplied name is empty or only whitespace.
#[utoipa::path(
	post,
	path = "/mint",
	operation_id = "calendar_tokens_mint",
	tag = "calendar_tokens",
	security(("tailscale-admin" = [])),
	request_body = MintCalendarArgs,
	responses(
		(status = 200, description = "Freshly minted feed; the URL in this response is shown once.", body = MintedCalendar),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn mint(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<MintCalendarArgs>,
) -> Result<Json<MintedCalendar>> {
	let name = args.name.trim();
	if name.is_empty() {
		return Err(commons_errors::AppError::BadRequest(
			"feed name cannot be empty".into(),
		));
	}
	let mut conn = state.db.get().await?;
	let (token, secret) = CalendarToken::mint(&mut conn, name, &admin.login).await?;
	Ok(Json(MintedCalendar {
		token: token.into(),
		url: feed_url(&secret),
		path: format!("/calendar/{secret}/upgrades.ics"),
	}))
}

/// Request body for revoking a calendar feed.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RevokeCalendarArgs {
	/// The id of the feed to revoke.
	pub id: Uuid,
}

/// Revoke a calendar feed.
///
/// Immediately stops the URL serving. Subscribers keep whatever their calendar
/// last fetched until they remove the subscription. Revoking an already-revoked
/// feed succeeds without doing anything further. Returns 404 if no feed with
/// that id exists.
#[utoipa::path(
	post,
	path = "/revoke",
	operation_id = "calendar_tokens_revoke",
	tag = "calendar_tokens",
	security(("tailscale-admin" = [])),
	request_body = RevokeCalendarArgs,
	responses(
		(status = 200, description = "Feed revoked (idempotent)."),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn revoke(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<RevokeCalendarArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	CalendarToken::revoke(&mut conn, args.id).await?;
	Ok(Json(()))
}
