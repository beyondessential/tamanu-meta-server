use axum::Json;
use axum::extract::State;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::{TailscaleAdmin, TailscaleUser};
use commons_types::{
	Uuid,
	issue::{ResolvedReason, Severity},
};
use database::issues::{
	Event, Incident, Issue, IssueFilter, IssueIncidentRef, IssueListFilters, NewEvent,
};
use database::notes::IssueNote;
use database::servers::Server;
use database::tailscale_users::TailscaleUser as CachedTailscaleUser;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::fns::Page;
use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueData {
	pub id: Uuid,
	pub server_id: Uuid,
	/// The issue's server name (may be null — fall back to `server_host`).
	pub server_name: Option<String>,
	pub server_host: String,
	/// Group id the issue's server belongs to; `None` when ungrouped. Used
	/// by the UI to offer group-scope actions (silence, etc.) without a
	/// second fetch.
	pub server_group_id: Option<Uuid>,
	/// Display name of the group the issue's server belongs to. `None` when
	/// the server is ungrouped; the UI hides the group prefix in that case.
	pub server_group_name: Option<String>,
	pub device_id: Option<Uuid>,
	pub source: String,
	#[serde(rename = "ref")]
	pub r#ref: String,
	pub severity: Severity,
	pub description: Option<String>,
	pub message: String,
	pub active: bool,
	pub first_seen: Timestamp,
	pub last_seen: Timestamp,
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolved_by_name: Option<String>,
	pub resolved_by_pic: Option<String>,
	/// The string stored in the DB; parses to `ResolvedReason` if valid.
	/// Kept as String to round-trip any historical value.
	pub resolved_reason: Option<String>,
	pub snoozed_until: Option<Timestamp>,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
	/// Distinct incidents this issue is or was attached to, most recent first.
	/// Empty for issues that never crossed the threshold to join an incident.
	pub incidents: Vec<IssueIncidentLink>,
}

/// Minimal incident reference attached to an issue, enough for the UI to
/// render a link and indicate open/closed status. See `Incident::for_issues`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueIncidentLink {
	pub incident_id: Uuid,
	pub opened_at: Timestamp,
	pub closed_at: Option<Timestamp>,
}

impl From<IssueIncidentRef> for IssueIncidentLink {
	fn from(r: IssueIncidentRef) -> Self {
		Self {
			incident_id: r.incident_id,
			opened_at: r.opened_at,
			closed_at: r.closed_at,
		}
	}
}

/// All the non-Issue extras we tuck into an `IssueData`.
struct IssueEnrichment<'a> {
	server_name: Option<String>,
	server_host: String,
	server_group_id: Option<Uuid>,
	server_group_name: Option<String>,
	users: &'a std::collections::HashMap<String, CachedTailscaleUser>,
	incidents: Vec<IssueIncidentLink>,
}

impl IssueData {
	fn from_with(i: Issue, e: IssueEnrichment<'_>) -> Self {
		let (res_name, res_pic) = lookup_user(e.users, i.resolved_by.as_deref());
		Self {
			id: i.id,
			server_id: i.server_id,
			server_name: e.server_name,
			server_host: e.server_host,
			server_group_id: e.server_group_id,
			server_group_name: e.server_group_name,
			device_id: i.device_id,
			source: i.source,
			r#ref: i.r#ref,
			severity: i.severity,
			description: i.description,
			message: i.message,
			active: i.active,
			first_seen: i.first_seen,
			last_seen: i.last_seen,
			resolved_at: i.resolved_at,
			resolved_by: i.resolved_by,
			resolved_by_name: res_name,
			resolved_by_pic: res_pic,
			resolved_reason: i.resolved_reason,
			snoozed_until: i.snoozed_until,
			created_at: i.created_at,
			updated_at: i.updated_at,
			incidents: e.incidents,
		}
	}
}

pub(crate) fn lookup_user(
	users: &std::collections::HashMap<String, CachedTailscaleUser>,
	login: Option<&str>,
) -> (Option<String>, Option<String>) {
	let Some(login) = login else {
		return (None, None);
	};
	match users.get(login) {
		Some(u) => (Some(u.name.clone()), u.profile_pic.clone()),
		None => (None, None),
	}
}

fn collect_user_logins(issues: &[Issue]) -> Vec<&str> {
	let mut s: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
	for i in issues {
		if let Some(l) = i.resolved_by.as_deref() {
			s.insert(l);
		}
	}
	s.into_iter().collect()
}

/// Enrich a list of issues with their server names/hosts, acker/resolver
/// display info, and the incidents each issue is attached to. Three extra
/// batch queries (servers, users, incidents).
pub(crate) async fn enrich_issues(
	conn: &mut database::diesel_async::AsyncPgConnection,
	issues: Vec<Issue>,
) -> Result<Vec<IssueData>> {
	let server_ids: Vec<Uuid> = issues.iter().map(|i| i.server_id).collect();
	let names = Server::names_by_ids(conn, &server_ids).await?;
	let group_refs = Server::group_refs_by_server_ids(conn, &server_ids).await?;
	let user_logins = collect_user_logins(&issues);
	let users = CachedTailscaleUser::by_logins(conn, &user_logins).await?;
	let issue_ids: Vec<Uuid> = issues.iter().map(|i| i.id).collect();
	let mut incidents = Incident::for_issues(conn, &issue_ids).await?;
	Ok(issues
		.into_iter()
		.map(|i| {
			let (name, host) = names.get(&i.server_id).cloned().unwrap_or((None, None));
			let (group_id, group_name) = group_refs
				.get(&i.server_id)
				.cloned()
				.unwrap_or((None, None));
			let links = incidents
				.remove(&i.id)
				.unwrap_or_default()
				.into_iter()
				.map(IssueIncidentLink::from)
				.collect();
			IssueData::from_with(
				i,
				IssueEnrichment {
					server_name: name,
					server_host: host.unwrap_or_default(),
					server_group_id: group_id,
					server_group_name: group_name,
					users: &users,
					incidents: links,
				},
			)
		})
		.collect())
}

/// Same as `enrich_issues` but for a single issue.
pub(crate) async fn enrich_issue(
	conn: &mut database::diesel_async::AsyncPgConnection,
	issue: Issue,
) -> Result<IssueData> {
	let mut names = Server::names_by_ids(conn, &[issue.server_id]).await?;
	let (name, host) = names.remove(&issue.server_id).unwrap_or((None, None));
	let mut group_refs = Server::group_refs_by_server_ids(conn, &[issue.server_id]).await?;
	let (group_id, group_name) = group_refs.remove(&issue.server_id).unwrap_or((None, None));
	let user_logins = collect_user_logins(std::slice::from_ref(&issue));
	let users = CachedTailscaleUser::by_logins(conn, &user_logins).await?;
	let mut incidents = Incident::for_issues(conn, &[issue.id]).await?;
	let links = incidents
		.remove(&issue.id)
		.unwrap_or_default()
		.into_iter()
		.map(IssueIncidentLink::from)
		.collect();
	Ok(IssueData::from_with(
		issue,
		IssueEnrichment {
			server_name: name,
			server_host: host.unwrap_or_default(),
			server_group_id: group_id,
			server_group_name: group_name,
			users: &users,
			incidents: links,
		},
	))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EventData {
	pub id: Uuid,
	pub issue_id: Uuid,
	pub created_at: Timestamp,
	pub occurred_at: Option<Timestamp>,
	pub severity: Severity,
	pub description: Option<String>,
	pub message: String,
	pub active: bool,
	pub occurrences: i32,
	pub last_seen: Timestamp,
}

impl From<Event> for EventData {
	fn from(e: Event) -> Self {
		Self {
			id: e.id,
			issue_id: e.issue_id,
			created_at: e.created_at,
			occurred_at: e.occurred_at,
			severity: e.severity,
			description: e.description,
			message: e.message,
			active: e.active,
			occurrences: e.occurrences,
			last_seen: e.last_seen,
		}
	}
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(list_for_device))
		.routes(routes!(list_for_server))
		.routes(routes!(list_events))
		.routes(routes!(submit_manual_event))
		.routes(routes!(resolve))
		.routes(routes!(unresolve))
		.routes(routes!(snooze))
		.routes(routes!(unsnooze))
		.routes(routes!(add_note))
		.routes(routes!(list_notes))
		.routes(routes!(delete_note))
}

fn filter_from(active_only: Option<bool>) -> IssueFilter {
	match active_only {
		Some(false) => IssueFilter::All,
		_ => IssueFilter::ActiveOnly,
	}
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListArgs {
	#[serde(default)]
	pub active_only: Option<bool>,
	#[serde(default)]
	pub severities: Option<Vec<Severity>>,
	#[serde(default)]
	pub server_group_id: Option<Uuid>,
	#[serde(default)]
	pub limit: Option<i64>,
}

/// Cross-server filtered issues list (used by the global Incidents page).
#[utoipa::path(
	post,
	path = "/list",
	operation_id = "issue_list",
	tag = "issues",
	security(("tailscale-user" = [])),
	request_body = ListArgs,
	responses(
		(status = 200, body = Vec<IssueData>),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ListArgs>,
) -> Result<Json<Vec<IssueData>>> {
	let mut conn = state.db.get().await?;
	let issues = Issue::list(
		&mut conn,
		IssueListFilters {
			active_only: args.active_only.unwrap_or(true),
			severities: args.severities,
			server_group_id: args.server_group_id,
		},
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(enrich_issues(&mut conn, issues).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ListForDeviceArgs {
	pub device_id: Uuid,
	#[serde(default)]
	pub active_only: Option<bool>,
	#[serde(default)]
	pub limit: Option<i64>,
}

#[utoipa::path(
	post,
	path = "/list_for_device",
	tag = "issues",
	security(("tailscale-user" = [])),
	request_body = ListForDeviceArgs,
	responses(
		(status = 200, body = Vec<IssueData>),
	),
)]
pub async fn list_for_device(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ListForDeviceArgs>,
) -> Result<Json<Vec<IssueData>>> {
	let mut conn = state.db.get().await?;
	let issues = Issue::list_for_device(
		&mut conn,
		args.device_id,
		filter_from(args.active_only),
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(enrich_issues(&mut conn, issues).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ListForServerArgs {
	pub server_id: Uuid,
	#[serde(default)]
	pub active_only: Option<bool>,
	#[serde(default)]
	pub limit: Option<i64>,
}

#[utoipa::path(
	post,
	path = "/list_for_server",
	operation_id = "issue_list_for_server",
	tag = "issues",
	security(("tailscale-user" = [])),
	request_body = ListForServerArgs,
	responses(
		(status = 200, body = Vec<IssueData>),
	),
)]
pub async fn list_for_server(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ListForServerArgs>,
) -> Result<Json<Vec<IssueData>>> {
	let mut conn = state.db.get().await?;
	let issues = Issue::list_for_server(
		&mut conn,
		args.server_id,
		filter_from(args.active_only),
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(enrich_issues(&mut conn, issues).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ListEventsArgs {
	pub issue_id: Uuid,
	#[serde(default)]
	pub offset: Option<i64>,
	#[serde(default)]
	pub limit: Option<i64>,
}

#[utoipa::path(
	post,
	path = "/list_events",
	tag = "issues",
	security(("tailscale-user" = [])),
	request_body = ListEventsArgs,
	responses(
		(status = 200, body = Page<EventData>),
	),
)]
pub async fn list_events(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ListEventsArgs>,
) -> Result<Json<Page<EventData>>> {
	let mut conn = state.db.get().await?;
	let total = Event::count_for_issue(&mut conn, args.issue_id).await? as u64;
	let events = Event::list_for_issue(
		&mut conn,
		args.issue_id,
		args.offset.unwrap_or(0),
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	let items = events.into_iter().map(EventData::from).collect();
	Ok(Json(Page { items, total }))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitManualEventArgs {
	pub server_id: Uuid,
	#[serde(rename = "ref")]
	pub r#ref: String,
	#[serde(default)]
	pub severity: Option<Severity>,
	#[serde(default)]
	pub description: Option<String>,
	pub message: String,
	#[serde(default)]
	pub active: Option<bool>,
	#[serde(default)]
	pub occurred_at: Option<Timestamp>,
}

#[utoipa::path(
	post,
	path = "/submit_manual_event",
	tag = "issues",
	security(("tailscale-admin" = [])),
	request_body = SubmitManualEventArgs,
	responses(
		(status = 200, body = IssueData),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn submit_manual_event(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SubmitManualEventArgs>,
) -> Result<Json<IssueData>> {
	if args.r#ref.trim().is_empty() {
		return Err(AppError::custom("ref is required"));
	}

	let event = NewEvent {
		source: "manual".to_string(),
		r#ref: args.r#ref,
		severity: args.severity,
		description: args.description,
		message: args.message,
		active: args.active,
		occurred_at: args.occurred_at,
	};
	let mut conn = state.db.get().await?;
	let issue = event.save(&mut conn, args.server_id, None).await?;
	Ok(Json(enrich_issue(&mut conn, issue).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct IssueIdArgs {
	pub issue_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct ResolveArgs {
	pub issue_id: Uuid,
	pub reason: ResolvedReason,
}

#[utoipa::path(
	post,
	path = "/resolve",
	operation_id = "issue_resolve",
	tag = "issues",
	security(("tailscale-admin" = [])),
	request_body = ResolveArgs,
	responses(
		(status = 200, body = IssueData),
	),
)]
pub async fn resolve(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<ResolveArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::resolve(&mut conn, args.issue_id, &admin.0.login, args.reason).await?;
	Ok(Json(enrich_issue(&mut conn, issue).await?))
}

#[utoipa::path(
	post,
	path = "/unresolve",
	operation_id = "issue_unresolve",
	tag = "issues",
	security(("tailscale-admin" = [])),
	request_body = IssueIdArgs,
	responses(
		(status = 200, body = IssueData),
	),
)]
pub async fn unresolve(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<IssueIdArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::unresolve(&mut conn, args.issue_id).await?;
	Ok(Json(enrich_issue(&mut conn, issue).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct SnoozeArgs {
	pub issue_id: Uuid,
	pub until: Timestamp,
}

#[utoipa::path(
	post,
	path = "/snooze",
	operation_id = "issue_snooze",
	tag = "issues",
	security(("tailscale-admin" = [])),
	request_body = SnoozeArgs,
	responses(
		(status = 200, body = IssueData),
	),
)]
pub async fn snooze(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<SnoozeArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::snooze(&mut conn, args.issue_id, args.until).await?;
	Ok(Json(enrich_issue(&mut conn, issue).await?))
}

#[utoipa::path(
	post,
	path = "/unsnooze",
	operation_id = "issue_unsnooze",
	tag = "issues",
	security(("tailscale-admin" = [])),
	request_body = IssueIdArgs,
	responses(
		(status = 200, body = IssueData),
	),
)]
pub async fn unsnooze(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<IssueIdArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::unsnooze(&mut conn, args.issue_id).await?;
	Ok(Json(enrich_issue(&mut conn, issue).await?))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueNoteData {
	pub id: Uuid,
	pub issue_id: Uuid,
	pub author: String,
	pub body: String,
	pub created_at: Timestamp,
}

impl From<IssueNote> for IssueNoteData {
	fn from(n: IssueNote) -> Self {
		Self {
			id: n.id,
			issue_id: n.issue_id,
			author: n.author,
			body: n.body,
			created_at: n.created_at,
		}
	}
}

#[derive(Deserialize, ToSchema)]
pub struct AddNoteArgs {
	pub issue_id: Uuid,
	pub body: String,
}

#[utoipa::path(
	post,
	path = "/add_note",
	operation_id = "issue_add_note",
	tag = "issues",
	security(("tailscale-admin" = [])),
	request_body = AddNoteArgs,
	responses(
		(status = 200, body = IssueNoteData),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn add_note(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<AddNoteArgs>,
) -> Result<Json<IssueNoteData>> {
	if args.body.trim().is_empty() {
		return Err(AppError::custom("note body is required"));
	}
	let mut conn = state.db.get().await?;
	let note = IssueNote::add(&mut conn, args.issue_id, &admin.0.login, &args.body).await?;
	Ok(Json(IssueNoteData::from(note)))
}

#[derive(Deserialize, ToSchema)]
pub struct ListNotesArgs {
	pub issue_id: Uuid,
	#[serde(default)]
	pub limit: Option<i64>,
}

#[utoipa::path(
	post,
	path = "/list_notes",
	operation_id = "issue_list_notes",
	tag = "issues",
	security(("tailscale-user" = [])),
	request_body = ListNotesArgs,
	responses(
		(status = 200, body = Vec<IssueNoteData>),
	),
)]
pub async fn list_notes(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<ListNotesArgs>,
) -> Result<Json<Vec<IssueNoteData>>> {
	let mut conn = state.db.get().await?;
	let notes = IssueNote::list_for_issue(
		&mut conn,
		args.issue_id,
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(notes.into_iter().map(IssueNoteData::from).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct DeleteNoteArgs {
	pub note_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/delete_note",
	operation_id = "issue_delete_note",
	tag = "issues",
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
	IssueNote::delete(&mut conn, args.note_id).await?;
	Ok(Json(()))
}
