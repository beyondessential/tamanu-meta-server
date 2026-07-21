//! Manual incident tools: the interface's one write surface, plus reads.
//!
//! `find_manual_incidents` / `get_manual_incident` read like any other
//! tool; `record_manual_incident` / `update_manual_incident` /
//! `delete_manual_incident` require the caller's [`crate::McpIdentity`]
//! (inserted by the mount's auth gate) to allow writes, and record its
//! identity as the author on creation.

use commons_types::Uuid;
use database::manual_incidents::{ManualIncident, ManualIncidentUpdate};
use jiff::Timestamp;
use rmcp::{
	handler::server::{common::Extension, wrapper::Parameters},
	model::{CallToolResult, ErrorData as McpError},
	tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
	CanopyMcp,
	util::{group_names, mcp_err, not_found, ok_json, parse_opt_uuid, parse_uuid, require_write},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindManualIncidentsArgs {
	/// Restrict to one group's id.
	pub group_id: Option<String>,
	/// Only incidents without an end time (still ongoing). Default false.
	pub ongoing_only: Option<bool>,
	/// Max incidents to return (default 100).
	pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ManualIncidentIdArgs {
	/// The manual incident's id.
	pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordManualIncidentArgs {
	/// Single-line headline for the incident.
	pub title: String,
	/// Markdown description: what happened, impact, resolution, links.
	pub description: Option<String>,
	/// When the incident started (RFC 3339, e.g. `2026-07-01T10:00:00Z`).
	pub started_at: String,
	/// When the incident ended (RFC 3339). Omit while it is ongoing.
	pub ended_at: Option<String>,
	/// Id of the affected server group.
	pub group_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateManualIncidentArgs {
	/// The manual incident's id.
	pub id: String,
	/// New headline. Omitted = unchanged.
	pub title: Option<String>,
	/// New markdown description. Omitted = unchanged.
	pub description: Option<String>,
	/// New start time (RFC 3339). Omitted = unchanged.
	pub started_at: Option<String>,
	/// New end time (RFC 3339). Omitted = unchanged.
	pub ended_at: Option<String>,
	/// Clear the end time, marking the incident ongoing again. Mutually
	/// exclusive with `ended_at`.
	pub clear_ended_at: Option<bool>,
	/// Id of a different affected server group. Omitted = unchanged.
	pub group_id: Option<String>,
}

#[derive(Serialize)]
struct ManualIncidentOut {
	id: Uuid,
	title: String,
	/// Markdown body; empty when nobody has written one yet.
	description: String,
	started_at: Timestamp,
	/// `null` while the incident is ongoing.
	ended_at: Option<Timestamp>,
	/// The affected server group.
	group_id: Uuid,
	group_name: String,
	/// Who recorded it: a tailnet login or an MCP token name.
	created_by: String,
	created_at: Timestamp,
	updated_at: Timestamp,
}

#[derive(Serialize)]
struct ManualIncidentList {
	count: usize,
	/// True when more incidents matched than `limit` allowed.
	truncated: bool,
	incidents: Vec<ManualIncidentOut>,
}

fn parse_timestamp(s: &str, field: &str) -> Result<Timestamp, McpError> {
	s.parse::<Timestamp>().map_err(|_| {
		McpError::invalid_params(
			format!("invalid {field}: {s} (want RFC 3339, e.g. 2026-07-01T10:00:00Z)"),
			None,
		)
	})
}

fn parse_opt_timestamp(v: &Option<String>, field: &str) -> Result<Option<Timestamp>, McpError> {
	v.as_deref().map(|s| parse_timestamp(s, field)).transpose()
}

impl CanopyMcp {
	async fn manual_incident_outs(
		&self,
		conn: &mut database::diesel_async::AsyncPgConnection,
		incidents: Vec<ManualIncident>,
	) -> Result<Vec<ManualIncidentOut>, McpError> {
		let group_ids: Vec<Uuid> = incidents.iter().map(|i| i.server_group_id).collect();
		let names = group_names(conn, &group_ids).await?;
		Ok(incidents
			.into_iter()
			.map(|i| ManualIncidentOut {
				id: i.id,
				title: i.title,
				description: i.description,
				started_at: i.started_at,
				ended_at: i.ended_at,
				group_id: i.server_group_id,
				group_name: names.get(&i.server_group_id).cloned().unwrap_or_default(),
				created_by: i.created_by,
				created_at: i.created_at,
				updated_at: i.updated_at,
			})
			.collect())
	}
}

#[tool_router(router = manual_incidents_router, vis = "pub(crate)")]
impl CanopyMcp {
	#[tool(
		description = "List manual incidents: support-recorded records of incidents managed by \
		               people, written after the fact — separate from the automatic incidents \
		               find_incidents returns. Most recently started first; optionally narrowed \
		               to one group or to ongoing ones."
	)]
	async fn find_manual_incidents(
		&self,
		Parameters(args): Parameters<FindManualIncidentsArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let group = parse_opt_uuid(&args.group_id, "group_id")?;
		let limit = args.limit.unwrap_or(100).clamp(1, 1000);
		let incidents = ManualIncident::list(
			&mut conn,
			group,
			args.ongoing_only.unwrap_or(false),
			limit + 1,
		)
		.await
		.map_err(mcp_err)?;
		let truncated = incidents.len() as i64 > limit;
		let incidents = incidents.into_iter().take(limit as usize).collect();
		let incidents = self.manual_incident_outs(&mut conn, incidents).await?;
		ok_json(&ManualIncidentList {
			count: incidents.len(),
			truncated,
			incidents,
		})
	}

	#[tool(description = "Fetch one manual incident by id.")]
	async fn get_manual_incident(
		&self,
		Parameters(args): Parameters<ManualIncidentIdArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let id = parse_uuid(&args.id, "id")?;
		let Some(incident) = ManualIncident::get(&mut conn, id).await.map_err(mcp_err)? else {
			return Ok(not_found(format!("no manual incident {id}")));
		};
		let out = self.manual_incident_outs(&mut conn, vec![incident]).await?;
		ok_json(&out[0])
	}

	#[tool(
		description = "Record a manual incident: a support-managed incident written after the \
		               fact. Takes a title, a start time, and the affected group; optionally a \
		               markdown description and an end time (omit while ongoing). The caller's \
		               identity is recorded as the author. Requires write access."
	)]
	async fn record_manual_incident(
		&self,
		Extension(parts): Extension<http::request::Parts>,
		Parameters(args): Parameters<RecordManualIncidentArgs>,
	) -> Result<CallToolResult, McpError> {
		let who = require_write(&parts)?;
		let started_at = parse_timestamp(&args.started_at, "started_at")?;
		let ended_at = parse_opt_timestamp(&args.ended_at, "ended_at")?;
		let group = parse_uuid(&args.group_id, "group_id")?;
		if args.title.trim().is_empty() {
			return Err(McpError::invalid_params("title is required", None));
		}

		let mut conn = self.write_conn().await?;
		if group_names(&mut conn, &[group]).await?.is_empty() {
			return Err(McpError::invalid_params(
				format!("no server group {group}"),
				None,
			));
		}
		let incident = ManualIncident::create(
			&mut conn,
			args.title.trim(),
			args.description.as_deref().unwrap_or_default(),
			started_at,
			ended_at,
			group,
			&who,
		)
		.await
		.map_err(mcp_err)?;
		tracing::info!(id = %incident.id, author = %who, "manual incident recorded");
		let out = self.manual_incident_outs(&mut conn, vec![incident]).await?;
		ok_json(&out[0])
	}

	#[tool(
		description = "Update a manual incident: any subset of title, description, start and end \
		               times, and affected group. `clear_ended_at` removes the end time, marking \
		               it ongoing again. Requires write access."
	)]
	async fn update_manual_incident(
		&self,
		Extension(parts): Extension<http::request::Parts>,
		Parameters(args): Parameters<UpdateManualIncidentArgs>,
	) -> Result<CallToolResult, McpError> {
		let who = require_write(&parts)?;
		let id = parse_uuid(&args.id, "id")?;
		let ended_at = parse_opt_timestamp(&args.ended_at, "ended_at")?;
		if args.clear_ended_at == Some(true) && ended_at.is_some() {
			return Err(McpError::invalid_params(
				"ended_at and clear_ended_at are mutually exclusive",
				None,
			));
		}
		if args.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
			return Err(McpError::invalid_params("title cannot be empty", None));
		}
		let group = parse_opt_uuid(&args.group_id, "group_id")?;

		let up = ManualIncidentUpdate {
			title: args.title.map(|t| t.trim().to_string()),
			description: args.description,
			started_at: parse_opt_timestamp(&args.started_at, "started_at")?,
			ended_at: if args.clear_ended_at == Some(true) {
				Some(None)
			} else {
				ended_at.map(Some)
			},
			server_group_id: group,
		};
		let mut conn = self.write_conn().await?;
		if let Some(group) = group
			&& group_names(&mut conn, &[group]).await?.is_empty()
		{
			return Err(McpError::invalid_params(
				format!("no server group {group}"),
				None,
			));
		}
		let Some(incident) = ManualIncident::update(&mut conn, id, up)
			.await
			.map_err(mcp_err)?
		else {
			return Ok(not_found(format!("no manual incident {id}")));
		};
		tracing::info!(id = %incident.id, author = %who, "manual incident updated");
		let out = self.manual_incident_outs(&mut conn, vec![incident]).await?;
		ok_json(&out[0])
	}

	#[tool(description = "Delete a manual incident by id. Requires write access.")]
	async fn delete_manual_incident(
		&self,
		Extension(parts): Extension<http::request::Parts>,
		Parameters(args): Parameters<ManualIncidentIdArgs>,
	) -> Result<CallToolResult, McpError> {
		let who = require_write(&parts)?;
		let id = parse_uuid(&args.id, "id")?;
		let mut conn = self.write_conn().await?;
		if !ManualIncident::delete(&mut conn, id)
			.await
			.map_err(mcp_err)?
		{
			return Ok(not_found(format!("no manual incident {id}")));
		}
		tracing::info!(%id, author = %who, "manual incident deleted");
		ok_json(&serde_json::json!({ "deleted": id }))
	}
}
