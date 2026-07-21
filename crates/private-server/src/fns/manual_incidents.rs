//! Read endpoints for manual incidents: support-recorded incident records.
//!
//! Spec: `.workhorse/specs/monitoring/incidents.md` (id `INC`), "Manual
//! incidents". Deliberately read-only — creating and editing happens over
//! the MCP interface; the operator UI only displays them.

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use commons_types::Uuid;
use database::manual_incidents::ManualIncident;
use database::server_groups::ServerGroup;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 100;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(get))
}

/// A support-recorded incident: written after the fact by people (over the
/// MCP interface), independent of the automatic incidents derived from
/// check state.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManualIncidentData {
	/// Unique identifier for this manual incident.
	pub id: Uuid,
	/// Single-line headline.
	pub title: String,
	/// Markdown body; empty when nobody has written one yet.
	pub description: String,
	/// When the incident started.
	pub started_at: Timestamp,
	/// When the incident ended; absent while it is ongoing.
	pub ended_at: Option<Timestamp>,
	/// Id of the affected server group. Absent for incidents concerning the
	/// fleet or Canopy generally.
	pub server_group_id: Option<Uuid>,
	/// Display name of the affected server group, when one is set.
	pub server_group_name: Option<String>,
	/// Who recorded it: a tailnet login or an MCP token name.
	pub created_by: String,
	/// When the record was created.
	pub created_at: Timestamp,
	/// When the record was last changed.
	pub updated_at: Timestamp,
}

async fn enrich(
	conn: &mut database::diesel_async::AsyncPgConnection,
	incidents: Vec<ManualIncident>,
) -> Result<Vec<ManualIncidentData>> {
	let group_ids: Vec<Uuid> = incidents.iter().filter_map(|i| i.server_group_id).collect();
	let names: std::collections::HashMap<Uuid, String> = ServerGroup::list_by_ids(conn, &group_ids)
		.await?
		.into_iter()
		.map(|g| (g.id, g.name))
		.collect();
	Ok(incidents
		.into_iter()
		.map(|i| ManualIncidentData {
			id: i.id,
			title: i.title,
			description: i.description,
			started_at: i.started_at,
			ended_at: i.ended_at,
			server_group_id: i.server_group_id,
			server_group_name: i.server_group_id.and_then(|id| names.get(&id).cloned()),
			created_by: i.created_by,
			created_at: i.created_at,
			updated_at: i.updated_at,
		})
		.collect())
}

/// Arguments for listing manual incidents.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManualIncidentListArgs {
	/// Restrict to one group's incidents.
	#[serde(default)]
	pub group_id: Option<Uuid>,
	/// Only incidents without an end time (still ongoing).
	#[serde(default)]
	pub ongoing_only: Option<bool>,
	/// Max incidents to return (default 100).
	#[serde(default)]
	pub limit: Option<i64>,
}

/// List manual incidents, most recently started first.
#[utoipa::path(
	post,
	path = "/list",
	operation_id = "manual_incidents_list",
	tag = "manual_incidents",
	security(("tailscale-user" = [])),
	request_body = ManualIncidentListArgs,
	responses(
		(status = 200, description = "Manual incidents, most recently started first.", body = Vec<ManualIncidentData>),
		(status = 401, body = ProblemDetailsSchema),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ManualIncidentListArgs>,
) -> Result<Json<Vec<ManualIncidentData>>> {
	let mut conn = state.db_read.get().await?;
	let incidents = ManualIncident::list(
		&mut conn,
		args.group_id,
		args.ongoing_only.unwrap_or(false),
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(enrich(&mut conn, incidents).await?))
}

/// Arguments identifying one manual incident.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManualIncidentGetArgs {
	/// Id of the manual incident to fetch.
	pub id: Uuid,
}

/// Fetch one manual incident by id.
#[utoipa::path(
	post,
	path = "/get",
	operation_id = "manual_incidents_get",
	tag = "manual_incidents",
	security(("tailscale-user" = [])),
	request_body = ManualIncidentGetArgs,
	responses(
		(status = 200, body = ManualIncidentData),
		(status = 401, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ManualIncidentGetArgs>,
) -> Result<Json<ManualIncidentData>> {
	let mut conn = state.db_read.get().await?;
	let incident = ManualIncident::get_required(&mut conn, args.id).await?;
	let mut enriched = enrich(&mut conn, vec![incident]).await?;
	Ok(Json(enriched.remove(0)))
}
