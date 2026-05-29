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
use database::servers::Server;
use database::statuses::Status;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(update))
		.routes(routes!(update_rules))
		.routes(routes!(sample))
		.routes(routes!(tag_keys))
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

#[derive(Deserialize, ToSchema)]
pub struct SampleArgs {
	pub check_name: String,
}

/// Materialised sample of the inputs available to a rule when this check
/// fails — fetched from the most recent status push (across all servers)
/// that reported `check_name`. The UI uses this to power autocomplete
/// suggestions, pass/warn validation on `var` input, and live previews
/// of a rule's effect against realistic data.
#[derive(Serialize, ToSchema)]
pub struct HealthcheckSample {
	/// Top-level status extras (`statuses.extra`).
	pub status_extra: serde_json::Map<String, JsonValue>,
	/// The failing check's own fields (`health[i]` minus `check` /
	/// `healthy`).
	pub check_extra: serde_json::Map<String, JsonValue>,
	/// Server's resolved tag map (server + group merge).
	pub tags: HashMap<String, String>,
	/// Server hostname for display.
	pub server_host: String,
	/// Optional friendly server name.
	pub server_name: Option<String>,
	/// When the sampled status push happened.
	pub seen_at: Timestamp,
}

#[derive(Serialize, ToSchema)]
pub struct HealthcheckSampleResponse {
	pub check_name: String,
	pub sample: Option<HealthcheckSample>,
}

#[utoipa::path(
	post,
	path = "/sample",
	operation_id = "healthcheck_sample",
	tag = "healthchecks",
	security(("tailscale-admin" = [])),
	request_body = SampleArgs,
	responses(
		(status = 200, description = "Sample payload or null if no server has reported this check yet.", body = HealthcheckSampleResponse),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn sample(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SampleArgs>,
) -> Result<Json<HealthcheckSampleResponse>> {
	let mut conn = state.db.get().await?;
	let Some(status) = Status::latest_for_check_name(&mut conn, &args.check_name).await? else {
		return Ok(Json(HealthcheckSampleResponse {
			check_name: args.check_name,
			sample: None,
		}));
	};
	let server = Server::get_by_id(&mut conn, status.server_id).await?;

	// Top-level extras — the column is always an object after our
	// ingestion path strips reserved keys.
	let status_extra = status.extra.as_object().cloned().unwrap_or_default();

	// Pull the failing-check entry out of the health array (any entry
	// matching by name; we don't require unhealthy here so we still
	// surface the check's typical shape even on a healthy push). Strip
	// the reserved fields so the UI sees only the operator-predicatable
	// extras — mirrors what the ingestion path passes to severity_for.
	let check_extra = status
		.health
		.as_array()
		.and_then(|arr| {
			arr.iter().find_map(|e| {
				let obj = e.as_object()?;
				let name = obj.get("check")?.as_str()?;
				if name == args.check_name {
					let mut m = obj.clone();
					m.remove("check");
					m.remove("healthy");
					Some(m)
				} else {
					None
				}
			})
		})
		.unwrap_or_default();

	let tag_map = server.tags_merged_with_group(&mut conn).await?;
	let tags: HashMap<String, String> = tag_map.0.into_iter().collect();

	Ok(Json(HealthcheckSampleResponse {
		check_name: args.check_name,
		sample: Some(HealthcheckSample {
			status_extra,
			check_extra,
			tags,
			server_host: server.host.0.to_string(),
			server_name: server.name,
			seen_at: status.created_at,
		}),
	}))
}

/// Distinct tag keys known anywhere in the system — union across all
/// servers and server groups, sorted. Feeds the rule editor's
/// Autocomplete so operators can pick `tag.<key>` even when the
/// sampled server doesn't carry that key. The sample-based pass/warn
/// badge still reflects the sample only; this list only widens the
/// completion menu.
#[utoipa::path(
	post,
	path = "/tag_keys",
	operation_id = "healthcheck_tag_keys",
	tag = "healthchecks",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, description = "Sorted, distinct tag keys.", body = Vec<String>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn tag_keys(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
) -> Result<Json<Vec<String>>> {
	let mut conn = state.db.get().await?;
	let keys = database::tags::all_known_keys(&mut conn).await?;
	Ok(Json(keys))
}
