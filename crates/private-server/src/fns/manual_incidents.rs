//! Endpoints for manual incidents: support-recorded incident records.
//!
//! Spec: `.workhorse/specs/monitoring/incidents.md` (id `INC`), "Manual
//! incidents". Writable both here (for the operator UI) and over the MCP
//! interface; either way every write is attributed — here to the tailnet
//! user making it.

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use commons_types::Uuid;
use database::manual_incidents::{ManualIncident, ManualIncidentUpdate};
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
		.routes(routes!(create))
		.routes(routes!(update))
		.routes(routes!(delete))
}

/// A support-recorded incident: written after the fact by people (in this
/// UI or over the MCP interface), independent of the automatic incidents
/// derived from check state.
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
	/// Id of the affected server group.
	pub server_group_id: Uuid,
	/// Display name of the affected server group.
	pub server_group_name: String,
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
	let group_ids: Vec<Uuid> = incidents.iter().map(|i| i.server_group_id).collect();
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
			server_group_name: names.get(&i.server_group_id).cloned().unwrap_or_default(),
			created_by: i.created_by,
			created_at: i.created_at,
			updated_at: i.updated_at,
		})
		.collect())
}

/// The affected group must exist before we write a record naming it.
async fn require_group(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group_id: Uuid,
) -> Result<()> {
	if ServerGroup::list_by_ids(conn, &[group_id])
		.await?
		.is_empty()
	{
		return Err(AppError::BadRequest(format!("no server group {group_id}")));
	}
	Ok(())
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
///
/// Manual incidents are support-recorded incident records, written in this
/// UI or over the MCP interface rather than derived from check state.
/// Optionally narrowed to one group or to ongoing incidents (those without
/// an end time).
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
///
/// Returns the full record, with the affected group's display name
/// resolved. Responds 404 if no manual incident has that id.
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

/// Arguments for recording a manual incident.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManualIncidentCreateArgs {
	/// Single-line headline.
	pub title: String,
	/// Markdown body. Defaults to empty.
	#[serde(default)]
	pub description: Option<String>,
	/// When the incident started.
	pub started_at: Timestamp,
	/// When the incident ended. Omit while it is ongoing.
	#[serde(default)]
	pub ended_at: Option<Timestamp>,
	/// Id of the affected server group.
	pub server_group_id: Uuid,
}

/// Record a manual incident.
///
/// Creates the record with the calling tailnet user as author and returns
/// it. The title must be non-empty and the affected group must exist.
#[utoipa::path(
	post,
	path = "/create",
	operation_id = "manual_incidents_create",
	tag = "manual_incidents",
	security(("tailscale-user" = [])),
	request_body = ManualIncidentCreateArgs,
	responses(
		(status = 200, body = ManualIncidentData),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	user: TailscaleUser,
	Json(args): Json<ManualIncidentCreateArgs>,
) -> Result<Json<ManualIncidentData>> {
	let title = args.title.trim();
	if title.is_empty() {
		return Err(AppError::BadRequest("title is required".into()));
	}

	let mut conn = state.db.get().await?;
	require_group(&mut conn, args.server_group_id).await?;
	let incident = ManualIncident::create(
		&mut conn,
		title,
		args.description.as_deref().unwrap_or_default(),
		args.started_at,
		args.ended_at,
		args.server_group_id,
		&user.login,
	)
	.await?;
	tracing::info!(id = %incident.id, author = %user.login, "manual incident recorded");
	let mut enriched = enrich(&mut conn, vec![incident]).await?;
	Ok(Json(enriched.remove(0)))
}

/// Field edits for one manual incident. Omitted fields are unchanged.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManualIncidentUpdateArgs {
	/// Id of the manual incident to edit.
	pub id: Uuid,
	/// New headline.
	#[serde(default)]
	pub title: Option<String>,
	/// New markdown body.
	#[serde(default)]
	pub description: Option<String>,
	/// New start time.
	#[serde(default)]
	pub started_at: Option<Timestamp>,
	/// New end time. Mutually exclusive with `clearEndedAt`.
	#[serde(default)]
	pub ended_at: Option<Timestamp>,
	/// Clear the end time, marking the incident ongoing again.
	#[serde(default)]
	pub clear_ended_at: Option<bool>,
	/// Id of a different affected server group.
	#[serde(default)]
	pub server_group_id: Option<Uuid>,
}

/// Update a manual incident.
///
/// Applies any subset of title, description, start and end times, and
/// affected group; `clearEndedAt` removes the end time, marking the
/// incident ongoing again. Responds 404 if no manual incident has that id.
#[utoipa::path(
	post,
	path = "/update",
	operation_id = "manual_incidents_update",
	tag = "manual_incidents",
	security(("tailscale-user" = [])),
	request_body = ManualIncidentUpdateArgs,
	responses(
		(status = 200, body = ManualIncidentData),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	user: TailscaleUser,
	Json(args): Json<ManualIncidentUpdateArgs>,
) -> Result<Json<ManualIncidentData>> {
	if args.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
		return Err(AppError::BadRequest("title cannot be empty".into()));
	}
	if args.clear_ended_at == Some(true) && args.ended_at.is_some() {
		return Err(AppError::BadRequest(
			"endedAt and clearEndedAt are mutually exclusive".into(),
		));
	}

	let mut conn = state.db.get().await?;
	if let Some(group_id) = args.server_group_id {
		require_group(&mut conn, group_id).await?;
	}
	let up = ManualIncidentUpdate {
		title: args.title.map(|t| t.trim().to_string()),
		description: args.description,
		started_at: args.started_at,
		ended_at: if args.clear_ended_at == Some(true) {
			Some(None)
		} else {
			args.ended_at.map(Some)
		},
		server_group_id: args.server_group_id,
	};
	// 404 on unknown ids; the update itself reports None for them.
	ManualIncident::get_required(&mut conn, args.id).await?;
	let incident = ManualIncident::update(&mut conn, args.id, up)
		.await?
		.ok_or_else(|| AppError::BadRequest("incident vanished mid-update".into()))?;
	tracing::info!(id = %incident.id, author = %user.login, "manual incident updated");
	let mut enriched = enrich(&mut conn, vec![incident]).await?;
	Ok(Json(enriched.remove(0)))
}

/// Delete a manual incident.
///
/// Removes the record entirely. Responds 404 if no manual incident has
/// that id.
#[utoipa::path(
	post,
	path = "/delete",
	operation_id = "manual_incidents_delete",
	tag = "manual_incidents",
	security(("tailscale-user" = [])),
	request_body = ManualIncidentGetArgs,
	responses(
		(status = 200, body = ()),
		(status = 401, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn delete(
	State(state): State<AppState>,
	user: TailscaleUser,
	Json(args): Json<ManualIncidentGetArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	// 404 on unknown ids; delete itself only reports a bool.
	ManualIncident::get_required(&mut conn, args.id).await?;
	ManualIncident::delete(&mut conn, args.id).await?;
	tracing::info!(id = %args.id, author = %user.login, "manual incident deleted");
	Ok(Json(()))
}
