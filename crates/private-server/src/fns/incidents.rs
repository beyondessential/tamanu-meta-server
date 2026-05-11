use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::Result;
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{Uuid, issue::ResolvedReason};
use database::issues::{Incident, IncidentIssue};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::fns::issues::IssueData;
use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentData {
	pub id: Uuid,
	pub server_id: Uuid,
	pub opened_at: Timestamp,
	pub closed_at: Option<Timestamp>,
	pub acknowledged_at: Option<Timestamp>,
	pub acknowledged_by: Option<String>,
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolved_reason: Option<String>,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
}

impl From<Incident> for IncidentData {
	fn from(i: Incident) -> Self {
		Self {
			id: i.id,
			server_id: i.server_id,
			opened_at: i.opened_at,
			closed_at: i.closed_at,
			acknowledged_at: i.acknowledged_at,
			acknowledged_by: i.acknowledged_by,
			resolved_at: i.resolved_at,
			resolved_by: i.resolved_by,
			resolved_reason: i.resolved_reason,
			created_at: i.created_at,
			updated_at: i.updated_at,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentIssueData {
	pub joined_at: Timestamp,
	pub left_at: Option<Timestamp>,
	pub issue: IssueData,
}

impl From<(IncidentIssue, IssueData)> for IncidentIssueData {
	fn from((link, issue): (IncidentIssue, IssueData)) -> Self {
		Self {
			joined_at: link.joined_at,
			left_at: link.left_at,
			issue,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentWithIssues {
	pub incident: IncidentData,
	pub issues: Vec<IncidentIssueData>,
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/list_for_server", post(list_for_server))
		.route("/list_active", post(list_active))
		.route("/get", post(get_incident))
		.route("/ack", post(ack))
		.route("/unack", post(unack))
		.route("/resolve", post(resolve))
		.route("/unresolve", post(unresolve))
}

#[derive(Deserialize)]
pub struct ListForServerArgs {
	pub server_id: Uuid,
	#[serde(default)]
	pub include_closed: Option<bool>,
	#[serde(default)]
	pub limit: Option<i64>,
}

pub async fn list_for_server(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<ListForServerArgs>,
) -> Result<Json<Vec<IncidentData>>> {
	let mut conn = state.db.get().await?;
	let incidents = Incident::list_for_server(
		&mut conn,
		args.server_id,
		args.include_closed.unwrap_or(false),
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(incidents.into_iter().map(IncidentData::from).collect()))
}

#[derive(Deserialize)]
pub struct ListActiveArgs {
	#[serde(default)]
	pub limit: Option<i64>,
}

pub async fn list_active(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<ListActiveArgs>,
) -> Result<Json<Vec<IncidentData>>> {
	let mut conn = state.db.get().await?;
	let incidents = Incident::list_active(&mut conn, args.limit.unwrap_or(DEFAULT_LIMIT)).await?;
	Ok(Json(incidents.into_iter().map(IncidentData::from).collect()))
}

#[derive(Deserialize)]
pub struct GetIncidentArgs {
	pub incident_id: Uuid,
}

pub async fn get_incident(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<GetIncidentArgs>,
) -> Result<Json<IncidentWithIssues>> {
	let mut conn = state.db.get().await?;
	let (incident, rows) = Incident::get_with_issues(&mut conn, args.incident_id).await?;
	let issues = rows
		.into_iter()
		.map(|(link, issue)| IncidentIssueData::from((link, IssueData::from(issue))))
		.collect();
	Ok(Json(IncidentWithIssues {
		incident: IncidentData::from(incident),
		issues,
	}))
}

#[derive(Deserialize)]
pub struct IncidentIdArgs {
	pub incident_id: Uuid,
}

pub async fn ack(
	State(state): State<AppState>,
	TailscaleAdmin(user): TailscaleAdmin,
	Json(args): Json<IncidentIdArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident = Incident::ack(&mut conn, args.incident_id, &user.login).await?;
	Ok(Json(IncidentData::from(incident)))
}

pub async fn unack(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<IncidentIdArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident = Incident::unack(&mut conn, args.incident_id).await?;
	Ok(Json(IncidentData::from(incident)))
}

#[derive(Deserialize)]
pub struct ResolveIncidentArgs {
	pub incident_id: Uuid,
	pub reason: ResolvedReason,
}

pub async fn resolve(
	State(state): State<AppState>,
	TailscaleAdmin(user): TailscaleAdmin,
	Json(args): Json<ResolveIncidentArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident = Incident::resolve(&mut conn, args.incident_id, &user.login, args.reason).await?;
	Ok(Json(IncidentData::from(incident)))
}

pub async fn unresolve(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<IncidentIdArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident = Incident::unresolve(&mut conn, args.incident_id).await?;
	Ok(Json(IncidentData::from(incident)))
}
