use axum::Json;
use axum::extract::State;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::{TailscaleAdmin, TailscaleUser};
use commons_types::{Uuid, issue::ResolvedReason};
use database::issues::{Incident, IncidentIssue, IncidentStats};
use database::notes::IncidentNote;
use database::servers::Server;
use database::tailscale_users::TailscaleUser as CachedTailscaleUser;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::fns::issues::{IssueData, enrich_issues, lookup_user};
use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IncidentData {
	pub id: Uuid,
	pub server_id: Uuid,
	/// The root server's name (may be null — fall back to `server_host`).
	pub server_name: Option<String>,
	pub server_host: String,
	pub opened_at: Timestamp,
	pub closed_at: Option<Timestamp>,
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolved_by_name: Option<String>,
	pub resolved_by_pic: Option<String>,
	pub resolved_reason: Option<String>,
	pub issue_count: i64,
	pub event_count: i64,
	/// Combined: this incident's notes + notes on all contributing issues.
	pub note_count: i64,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
}

impl IncidentData {
	fn from_with(
		i: Incident,
		server_name: Option<String>,
		server_host: String,
		users: &std::collections::HashMap<String, CachedTailscaleUser>,
		stats: IncidentStats,
	) -> Self {
		let (res_name, res_pic) = lookup_user(users, i.resolved_by.as_deref());
		Self {
			id: i.id,
			server_id: i.server_id,
			server_name,
			server_host,
			opened_at: i.opened_at,
			closed_at: i.closed_at,
			resolved_at: i.resolved_at,
			resolved_by: i.resolved_by,
			resolved_by_name: res_name,
			resolved_by_pic: res_pic,
			resolved_reason: i.resolved_reason,
			issue_count: stats.issue_count,
			event_count: stats.event_count,
			note_count: stats.note_count,
			created_at: i.created_at,
			updated_at: i.updated_at,
		}
	}
}

fn collect_incident_user_logins(incidents: &[Incident]) -> Vec<&str> {
	let mut s: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
	for i in incidents {
		if let Some(l) = i.resolved_by.as_deref() {
			s.insert(l);
		}
	}
	s.into_iter().collect()
}

async fn enrich_incidents(
	conn: &mut database::diesel_async::AsyncPgConnection,
	pool: &database::Db,
	incidents: Vec<Incident>,
) -> Result<Vec<IncidentData>> {
	let server_ids: Vec<Uuid> = incidents.iter().map(|i| i.server_id).collect();
	let incident_ids: Vec<Uuid> = incidents.iter().map(|i| i.id).collect();
	let names = Server::names_by_ids(conn, &server_ids).await?;
	let user_logins = collect_incident_user_logins(&incidents);
	let users = CachedTailscaleUser::by_logins(conn, &user_logins).await?;
	let stats = Incident::stats_for(pool, &incident_ids).await?;
	Ok(incidents
		.into_iter()
		.map(|i| {
			let (name, host) = names
				.get(&i.server_id)
				.cloned()
				.unwrap_or((None, String::new()));
			let s = stats.get(&i.id).copied().unwrap_or_default();
			IncidentData::from_with(i, name, host, &users, s)
		})
		.collect())
}

async fn enrich_incident(
	conn: &mut database::diesel_async::AsyncPgConnection,
	pool: &database::Db,
	incident: Incident,
) -> Result<IncidentData> {
	let mut names = Server::names_by_ids(conn, &[incident.server_id]).await?;
	let (name, host) = names
		.remove(&incident.server_id)
		.unwrap_or((None, String::new()));
	let user_logins = collect_incident_user_logins(std::slice::from_ref(&incident));
	let users = CachedTailscaleUser::by_logins(conn, &user_logins).await?;
	let mut stats = Incident::stats_for(pool, &[incident.id]).await?;
	let s = stats.remove(&incident.id).unwrap_or_default();
	Ok(IncidentData::from_with(incident, name, host, &users, s))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IncidentWithIssues {
	pub incident: IncidentData,
	pub issues: Vec<IncidentIssueData>,
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list_for_server))
		.routes(routes!(list_active))
		.routes(routes!(get_incident))
		.routes(routes!(resolve))
		.routes(routes!(unresolve))
		.routes(routes!(add_note))
		.routes(routes!(list_notes))
		.routes(routes!(delete_note))
}

#[derive(Deserialize, ToSchema)]
pub struct ListForServerArgs {
	pub server_id: Uuid,
	#[serde(default)]
	pub include_closed: Option<bool>,
	#[serde(default)]
	pub limit: Option<i64>,
}

#[utoipa::path(
	post,
	path = "/list_for_server",
	operation_id = "incident_list_for_server",
	tag = "incidents",
	security(("tailscale-user" = [])),
	request_body = ListForServerArgs,
	responses(
		(status = 200, body = Vec<IncidentData>),
	),
)]
pub async fn list_for_server(
	State(state): State<AppState>,
	_user: TailscaleUser,
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
	Ok(Json(
		enrich_incidents(&mut conn, &state.db, incidents).await?,
	))
}

#[derive(Deserialize, ToSchema)]
pub struct ListActiveArgs {
	#[serde(default)]
	pub limit: Option<i64>,
}

#[utoipa::path(
	post,
	path = "/list_active",
	tag = "incidents",
	security(("tailscale-user" = [])),
	request_body = ListActiveArgs,
	responses(
		(status = 200, body = Vec<IncidentData>),
	),
)]
pub async fn list_active(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ListActiveArgs>,
) -> Result<Json<Vec<IncidentData>>> {
	let mut conn = state.db.get().await?;
	let incidents = Incident::list_active(&mut conn, args.limit.unwrap_or(DEFAULT_LIMIT)).await?;
	Ok(Json(
		enrich_incidents(&mut conn, &state.db, incidents).await?,
	))
}

#[derive(Deserialize, ToSchema)]
pub struct GetIncidentArgs {
	pub incident_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/get",
	tag = "incidents",
	security(("tailscale-user" = [])),
	request_body = GetIncidentArgs,
	responses(
		(status = 200, body = IncidentWithIssues),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn get_incident(
	State(state): State<AppState>,
	_user: TailscaleUser,
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
	let incident = enrich_incident(&mut conn, &state.db, incident).await?;
	Ok(Json(IncidentWithIssues { incident, issues }))
}

#[derive(Deserialize, ToSchema)]
pub struct IncidentIdArgs {
	pub incident_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct ResolveIncidentArgs {
	pub incident_id: Uuid,
	pub reason: ResolvedReason,
}

#[utoipa::path(
	post,
	path = "/resolve",
	operation_id = "incident_resolve",
	tag = "incidents",
	security(("tailscale-admin" = [])),
	request_body = ResolveIncidentArgs,
	responses(
		(status = 200, body = IncidentData),
	),
)]
pub async fn resolve(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<ResolveIncidentArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident =
		Incident::resolve(&mut conn, args.incident_id, &admin.0.login, args.reason).await?;
	Ok(Json(enrich_incident(&mut conn, &state.db, incident).await?))
}

#[utoipa::path(
	post,
	path = "/unresolve",
	operation_id = "incident_unresolve",
	tag = "incidents",
	security(("tailscale-admin" = [])),
	request_body = IncidentIdArgs,
	responses(
		(status = 200, body = IncidentData),
	),
)]
pub async fn unresolve(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<IncidentIdArgs>,
) -> Result<Json<IncidentData>> {
	let mut conn = state.db.get().await?;
	let incident = Incident::unresolve(&mut conn, args.incident_id).await?;
	Ok(Json(enrich_incident(&mut conn, &state.db, incident).await?))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Deserialize, ToSchema)]
pub struct AddNoteArgs {
	pub incident_id: Uuid,
	pub body: String,
}

#[utoipa::path(
	post,
	path = "/add_note",
	operation_id = "incident_add_note",
	tag = "incidents",
	security(("tailscale-admin" = [])),
	request_body = AddNoteArgs,
	responses(
		(status = 200, body = IncidentNoteData),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn add_note(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<AddNoteArgs>,
) -> Result<Json<IncidentNoteData>> {
	if args.body.trim().is_empty() {
		return Err(AppError::custom("note body is required"));
	}
	let mut conn = state.db.get().await?;
	let note = IncidentNote::add(&mut conn, args.incident_id, &admin.0.login, &args.body).await?;
	Ok(Json(IncidentNoteData::from(note)))
}

#[derive(Deserialize, ToSchema)]
pub struct ListNotesArgs {
	pub incident_id: Uuid,
	#[serde(default)]
	pub limit: Option<i64>,
}

#[utoipa::path(
	post,
	path = "/list_notes",
	operation_id = "incident_list_notes",
	tag = "incidents",
	security(("tailscale-user" = [])),
	request_body = ListNotesArgs,
	responses(
		(status = 200, body = Vec<IncidentNoteData>),
	),
)]
pub async fn list_notes(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ListNotesArgs>,
) -> Result<Json<Vec<IncidentNoteData>>> {
	let mut conn = state.db.get().await?;
	let notes = IncidentNote::list_for_incident(
		&mut conn,
		args.incident_id,
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(
		notes.into_iter().map(IncidentNoteData::from).collect(),
	))
}

#[derive(Deserialize, ToSchema)]
pub struct DeleteNoteArgs {
	pub note_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/delete_note",
	operation_id = "incident_delete_note",
	tag = "incidents",
	security(("tailscale-admin" = [])),
	request_body = DeleteNoteArgs,
	responses(
		(status = 200),
	),
)]
pub async fn delete_note(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeleteNoteArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	IncidentNote::delete(&mut conn, args.note_id).await?;
	Ok(Json(()))
}
