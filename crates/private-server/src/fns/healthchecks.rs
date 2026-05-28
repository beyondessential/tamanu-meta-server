//! Operator-owned catalog of healthcheck names → severity. Read and
//! edit endpoints for the catalog page at /healthchecks. Ingestion
//! (in the public-server status handler) maintains the rows; this
//! module exposes them to admins.

use axum::Json;
use axum::extract::State;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::issue::Severity;
use database::healthcheck_severities::{HealthcheckSeverity, IfLadder};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(update))
		.routes(routes!(update_rules))
}

/// Catalog row enriched with `pending_review` and `rule_count` derivations
/// for the UI. `rules` is the raw JsonLogic blob exactly as stored; the
/// React side parses it client-side (or the per-check editor rebuilds it
/// from form state and POSTs to /update_rules). Malformed `rules` parses
/// to `rule_count: 0` and the row behaves as "no conditional rules" — the
/// evaluator does the same on the ingestion path.
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
	/// JsonLogic if-ladder; `null` means no conditional rules.
	#[schema(value_type = Option<serde_json::Value>)]
	pub rules: Option<JsonValue>,
	/// Number of branches in the rules ladder; 0 when `rules` is null
	/// or malformed. The main /healthchecks page uses this to decide
	/// between the simple severity dropdown and the "Custom rules" link.
	pub rule_count: u32,
}

fn rule_count(rules: &Option<JsonValue>) -> u32 {
	let Some(v) = rules else { return 0 };
	serde_json::from_value::<IfLadder>(v.clone())
		.map(|l| l.branches.len() as u32)
		.unwrap_or(0)
}

impl From<HealthcheckSeverity> for HealthcheckSeverityData {
	fn from(h: HealthcheckSeverity) -> Self {
		let pending_review = h.reviewed_at.is_none();
		let rule_count = rule_count(&h.rules);
		Self {
			check_name: h.check_name,
			severity: h.severity,
			first_seen: h.first_seen,
			reviewed_at: h.reviewed_at,
			reviewed_by: h.reviewed_by,
			notes: h.notes,
			updated_at: h.updated_at,
			pending_review,
			rules: h.rules,
			rule_count,
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

#[derive(Deserialize, ToSchema)]
pub struct UpdateRulesArgs {
	pub check_name: String,
	/// Either `null` (clear the ladder) or a JsonLogic if-ladder as
	/// documented on `IfLadder`. An empty-branches ladder is normalised
	/// to null at the API layer.
	#[schema(value_type = Option<serde_json::Value>)]
	#[serde(default)]
	pub rules: Option<JsonValue>,
}

#[utoipa::path(
	post,
	path = "/update_rules",
	operation_id = "healthcheck_update_rules",
	tag = "healthchecks",
	security(("tailscale-admin" = [])),
	request_body = UpdateRulesArgs,
	responses(
		(status = 200, description = "Updated catalog row.", body = HealthcheckSeverityData),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn update_rules(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<UpdateRulesArgs>,
) -> Result<Json<HealthcheckSeverityData>> {
	let ladder: Option<IfLadder> = match args.rules {
		None | Some(JsonValue::Null) => None,
		Some(v) => {
			let parsed: IfLadder = serde_json::from_value(v)
				.map_err(|e| AppError::BadRequest(format!("invalid rules: {e}")))?;
			// An empty ladder is equivalent to "no rules"; normalise so the
			// stored shape is always either NULL or a non-empty if-ladder.
			if parsed.branches.is_empty() {
				None
			} else {
				Some(parsed)
			}
		}
	};
	let mut conn = state.db.get().await?;
	let row = HealthcheckSeverity::update_rules(
		&mut conn,
		&args.check_name,
		ladder.as_ref(),
		&admin.0.login,
	)
	.await?;
	Ok(Json(row.into()))
}
