use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{Uuid, issue::ResolvedReason};
use database::issues::{Incident, IncidentIssue};
use database::notes::IncidentNote;
use database::servers::Server;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::fns::issues::{IssueData, enrich_issues};
use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentData {
	pub id: Uuid,
	pub server_id: Uuid,
	/// The root server's name (may be null — fall back to `server_host`).
	pub server_name: Option<String>,
	pub server_host: String,
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

impl IncidentData {
	fn from_with(i: Incident, server_name: Option<String>, server_host: String) -> Self {
		Self {
			id: i.id,
			server_id: i.server_id,
			server_name,
			server_host,
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

async fn enrich_incidents(
	conn: &mut database::diesel_async::AsyncPgConnection,
	incidents: Vec<Incident>,
) -> Result<Vec<IncidentData>> {
	let ids: Vec<Uuid> = incidents.iter().map(|i| i.server_id).collect();
	let names = Server::names_by_ids(conn, &ids).await?;
	Ok(incidents
		.into_iter()
		.map(|i| {
			let (name, host) = names
				.get(&i.server_id)
				.cloned()
				.unwrap_or((None, String::new()));
			IncidentData::from_with(i, name, host)
		})
		.collect())
}

async fn enrich_incident(
	conn: &mut database::diesel_async::AsyncPgConnection,
	incident: Incident,
) -> Result<IncidentData> {
	let mut names = Server::names_by_ids(conn, &[incident.server_id]).await?;
	let (name, host) = names
		.remove(&incident.server_id)
		.unwrap_or((None, String::new()));
	Ok(IncidentData::from_with(incident, name, host))
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
		.route("/add_note", post(add_note))
		.route("/list_notes", post(list_notes))
		.route("/delete_note", post(delete_note))
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
	Ok(Json(enrich_incidents(&mut conn, incidents).await?))
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
	Ok(Json(enrich_incidents(&mut conn, incidents).await?))
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
	let (links, raw_issues): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
	let issue_data = enrich_issues(&mut conn, raw_issues).await?;
	let issues = links
		.into_iter()
		.zip(issue_data)
		.map(|(link, issue)| IncidentIssueData::from((link, issue)))
		.collect();
	let incident = enrich_incident(&mut conn, incident).await?;
	Ok(Json(IncidentWithIssues { incident, issues }))
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
	Ok(Json(enrich_incident(&mut conn, incident).await?))
}

pub async fn unack(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<IncidentIdArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident = Incident::unack(&mut conn, args.incident_id).await?;
	Ok(Json(enrich_incident(&mut conn, incident).await?))
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
	Ok(Json(enrich_incident(&mut conn, incident).await?))
}

pub async fn unresolve(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<IncidentIdArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident = Incident::unresolve(&mut conn, args.incident_id).await?;
	Ok(Json(enrich_incident(&mut conn, incident).await?))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentNoteData {
	pub id: Uuid,
	pub incident_id: Uuid,
	pub author: String,
	pub body: String,
	pub created_at: Timestamp,
}

impl From<IncidentNote> for IncidentNoteData {
	fn from(n: IncidentNote) -> Self {
		Self {
			id: n.id,
			incident_id: n.incident_id,
			author: n.author,
			body: n.body,
			created_at: n.created_at,
		}
	}
}

#[derive(Deserialize)]
pub struct AddNoteArgs {
	pub incident_id: Uuid,
	pub body: String,
}

pub async fn add_note(
	State(state): State<AppState>,
	TailscaleAdmin(user): TailscaleAdmin,
	Json(args): Json<AddNoteArgs>,
) -> Result<Json<IncidentNoteData>> {
	if args.body.trim().is_empty() {
		return Err(AppError::custom("note body is required"));
	}
	let mut conn = state.db.get().await?;
	let note = IncidentNote::add(&mut conn, args.incident_id, &user.login, &args.body).await?;
	Ok(Json(IncidentNoteData::from(note)))
}

#[derive(Deserialize)]
pub struct ListNotesArgs {
	pub incident_id: Uuid,
	#[serde(default)]
	pub limit: Option<i64>,
}

pub async fn list_notes(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<ListNotesArgs>,
) -> Result<Json<Vec<IncidentNoteData>>> {
	let mut conn = state.db.get().await?;
	let notes = IncidentNote::list_for_incident(
		&mut conn,
		args.incident_id,
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(notes.into_iter().map(IncidentNoteData::from).collect()))
}

#[derive(Deserialize)]
pub struct DeleteNoteArgs {
	pub note_id: Uuid,
}

pub async fn delete_note(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<DeleteNoteArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	IncidentNote::delete(&mut conn, args.note_id).await?;
	Ok(Json(()))
}
