//! Operator-owned catalog of healthcheck names → severity. Read and
//! edit endpoints for the catalog page at /healthchecks. Ingestion
//! (in the public-server status handler) maintains the rows; this
//! module exposes them to admins.

use axum::Json;
use axum::extract::State;
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::issue::Severity;
use database::healthcheck_severities::HealthcheckSeverity;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(update))
}

/// Catalog row enriched with a `pending_review` flag for the UI.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HealthcheckSeverityData {
	pub check_name: String,
	pub severity: Severity,
	pub first_seen: Timestamp,
	pub reviewed_at: Option<Timestamp>,
	pub reviewed_by: Option<String>,
	pub notes: Option<String>,
	pub updated_at: Timestamp,
	/// `true` when no operator has confirmed this row yet
	/// (`reviewed_at IS NULL`). The catalog UI surfaces these.
	pub pending_review: bool,
}

impl From<HealthcheckSeverity> for HealthcheckSeverityData {
	fn from(h: HealthcheckSeverity) -> Self {
		let pending_review = h.reviewed_at.is_none();
		Self {
			check_name: h.check_name,
			severity: h.severity,
			first_seen: h.first_seen,
			reviewed_at: h.reviewed_at,
			reviewed_by: h.reviewed_by,
			notes: h.notes,
			updated_at: h.updated_at,
			pending_review,
		}
	}
}

#[utoipa::path(
	post,
	path = "/list",
	operation_id = "healthcheck_list",
	tag = "healthchecks",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, description = "Catalog rows ordered by check_name.", body = Vec<HealthcheckSeverityData>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
) -> Result<Json<Vec<HealthcheckSeverityData>>> {
	let mut conn = state.db.get().await?;
	let rows = HealthcheckSeverity::list(&mut conn).await?;
	Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateArgs {
	pub check_name: String,
	pub severity: Severity,
	/// Optional operator notes. `None` leaves the existing notes alone…
	/// well, actually it overwrites with NULL — pass the current value
	/// to preserve. The UI sends the full current state.
	#[serde(default)]
	pub notes: Option<String>,
}

#[utoipa::path(
	post,
	path = "/update",
	operation_id = "healthcheck_update",
	tag = "healthchecks",
	security(("tailscale-admin" = [])),
	request_body = UpdateArgs,
	responses(
		(status = 200, description = "Updated catalog row.", body = HealthcheckSeverityData),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<UpdateArgs>,
) -> Result<Json<HealthcheckSeverityData>> {
	let mut conn = state.db.get().await?;
	let row = HealthcheckSeverity::update(
		&mut conn,
		&args.check_name,
		args.severity,
		args.notes.as_deref(),
		&admin.0.login,
	)
	.await?;
	Ok(Json(row.into()))
}
