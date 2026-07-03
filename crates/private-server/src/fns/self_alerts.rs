//! Self-alerts for the operator UI: canopy's problems with its own
//! operation, presented apart from fleet issues.
//!
//! Spec: `.workhorse/specs/private-server/self-alerts.md` (id `SELF`).

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::{TailscaleAdmin, TailscaleUser};
use commons_types::Uuid;
use commons_types::issue::{ResolvedReason, Severity};
use database::issues::Issue;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(active))
		.routes(routes!(list))
		.routes(routes!(resolve))
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SelfAlertView {
	pub id: Uuid,
	/// The stable condition key, e.g. `mcp-token-expiry`.
	pub r#ref: String,
	pub severity: Severity,
	/// Single-line headline.
	pub title: Option<String>,
	pub message: String,
	pub active: bool,
	pub first_seen: Timestamp,
	pub last_seen: Timestamp,
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
}

impl From<Issue> for SelfAlertView {
	fn from(i: Issue) -> Self {
		Self {
			id: i.id,
			r#ref: i.r#ref,
			severity: i.severity,
			title: i.description,
			message: i.message,
			active: i.active,
			first_seen: i.first_seen,
			last_seen: i.last_seen,
			resolved_at: i.resolved_at,
			resolved_by: i.resolved_by,
		}
	}
}

#[utoipa::path(
	post,
	path = "/active",
	operation_id = "self_alerts_active",
	tag = "self_alerts",
	security(("tailscale-user" = [])),
	responses(
		(status = 200, description = "Currently-alerting self-alerts (active and not operator-resolved), for the banner.", body = Vec<SelfAlertView>),
		(status = 401, body = ProblemDetailsSchema),
	),
)]
pub async fn active(
	State(state): State<AppState>,
	_user: TailscaleUser,
) -> Result<Json<Vec<SelfAlertView>>> {
	let mut conn = state.db.get().await?;
	let alerts = database::self_alerts::list(&mut conn, 50)
		.await?
		.into_iter()
		.filter(|i| i.active && i.resolved_at.is_none())
		.map(Into::into)
		.collect();
	Ok(Json(alerts))
}

#[utoipa::path(
	post,
	path = "/list",
	operation_id = "self_alerts_list",
	tag = "self_alerts",
	security(("tailscale-user" = [])),
	responses(
		(status = 200, description = "Self-alerts, newest activity first, recovered/resolved included.", body = Vec<SelfAlertView>),
		(status = 401, body = ProblemDetailsSchema),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_user: TailscaleUser,
) -> Result<Json<Vec<SelfAlertView>>> {
	let mut conn = state.db.get().await?;
	let alerts = database::self_alerts::list(&mut conn, 50)
		.await?
		.into_iter()
		.map(Into::into)
		.collect();
	Ok(Json(alerts))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SelfAlertsResolveArgs {
	pub id: Uuid,
}

#[utoipa::path(
	post,
	path = "/resolve",
	operation_id = "self_alerts_resolve",
	tag = "self_alerts",
	security(("tailscale-admin" = [])),
	request_body = SelfAlertsResolveArgs,
	responses(
		(status = 200, description = "Alert marked operator-resolved; a pending notification is cancelled."),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn resolve(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<SelfAlertsResolveArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Issue::resolve(&mut conn, args.id, &admin.login, ResolvedReason::Fixed).await?;
	// If the alert's Slack open is still inside its grace window, the
	// operator has dealt with it before anyone needed paging.
	database::slack_outbox::SlackOutbox::cancel_pending_self_alert_open(
		&mut conn,
		args.id,
		"cancelled: self-alert operator-resolved before the open had been delivered to Slack",
	)
	.await?;
	Ok(Json(()))
}
