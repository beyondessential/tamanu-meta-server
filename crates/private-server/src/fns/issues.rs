use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{
	Uuid,
	issue::{ResolvedReason, Severity},
};
use database::issues::{Event, Issue, IssueFilter, NewEvent};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueData {
	pub id: Uuid,
	pub server_id: Uuid,
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
	pub acknowledged_at: Option<Timestamp>,
	pub acknowledged_by: Option<String>,
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	/// The string stored in the DB; parses to `ResolvedReason` if valid.
	/// Kept as String to round-trip any historical value.
	pub resolved_reason: Option<String>,
	pub snoozed_until: Option<Timestamp>,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
}

impl From<Issue> for IssueData {
	fn from(i: Issue) -> Self {
		Self {
			id: i.id,
			server_id: i.server_id,
			device_id: i.device_id,
			source: i.source,
			r#ref: i.r#ref,
			severity: i.severity,
			description: i.description,
			message: i.message,
			active: i.active,
			first_seen: i.first_seen,
			last_seen: i.last_seen,
			acknowledged_at: i.acknowledged_at,
			acknowledged_by: i.acknowledged_by,
			resolved_at: i.resolved_at,
			resolved_by: i.resolved_by,
			resolved_reason: i.resolved_reason,
			snoozed_until: i.snoozed_until,
			created_at: i.created_at,
			updated_at: i.updated_at,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/list_for_device", post(list_for_device))
		.route("/list_for_server", post(list_for_server))
		.route("/list_events", post(list_events))
		.route("/submit_manual_event", post(submit_manual_event))
		.route("/ack", post(ack))
		.route("/unack", post(unack))
		.route("/resolve", post(resolve))
		.route("/unresolve", post(unresolve))
		.route("/snooze", post(snooze))
		.route("/unsnooze", post(unsnooze))
}

fn filter_from(active_only: Option<bool>) -> IssueFilter {
	match active_only {
		Some(false) => IssueFilter::All,
		_ => IssueFilter::ActiveOnly,
	}
}

#[derive(Deserialize)]
pub struct ListForDeviceArgs {
	pub device_id: Uuid,
	#[serde(default)]
	pub active_only: Option<bool>,
	#[serde(default)]
	pub limit: Option<i64>,
}

pub async fn list_for_device(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
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
	Ok(Json(issues.into_iter().map(IssueData::from).collect()))
}

#[derive(Deserialize)]
pub struct ListForServerArgs {
	pub server_id: Uuid,
	#[serde(default)]
	pub active_only: Option<bool>,
	#[serde(default)]
	pub limit: Option<i64>,
}

pub async fn list_for_server(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
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
	Ok(Json(issues.into_iter().map(IssueData::from).collect()))
}

#[derive(Deserialize)]
pub struct ListEventsArgs {
	pub issue_id: Uuid,
	#[serde(default)]
	pub limit: Option<i64>,
}

pub async fn list_events(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<ListEventsArgs>,
) -> Result<Json<Vec<EventData>>> {
	let mut conn = state.db.get().await?;
	let events = Event::list_for_issue(
		&mut conn,
		args.issue_id,
		args.limit.unwrap_or(DEFAULT_LIMIT),
	)
	.await?;
	Ok(Json(events.into_iter().map(EventData::from).collect()))
}

#[derive(Deserialize)]
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

pub async fn submit_manual_event(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
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
	Ok(Json(IssueData::from(issue)))
}

#[derive(Deserialize)]
pub struct IssueIdArgs {
	pub issue_id: Uuid,
}

pub async fn ack(
	State(state): State<AppState>,
	TailscaleAdmin(user): TailscaleAdmin,
	Json(args): Json<IssueIdArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::ack(&mut conn, args.issue_id, &user.login).await?;
	Ok(Json(IssueData::from(issue)))
}

pub async fn unack(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<IssueIdArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::unack(&mut conn, args.issue_id).await?;
	Ok(Json(IssueData::from(issue)))
}

#[derive(Deserialize)]
pub struct ResolveArgs {
	pub issue_id: Uuid,
	pub reason: ResolvedReason,
}

pub async fn resolve(
	State(state): State<AppState>,
	TailscaleAdmin(user): TailscaleAdmin,
	Json(args): Json<ResolveArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::resolve(&mut conn, args.issue_id, &user.login, args.reason).await?;
	Ok(Json(IssueData::from(issue)))
}

pub async fn unresolve(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<IssueIdArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::unresolve(&mut conn, args.issue_id).await?;
	Ok(Json(IssueData::from(issue)))
}

#[derive(Deserialize)]
pub struct SnoozeArgs {
	pub issue_id: Uuid,
	pub until: Timestamp,
}

pub async fn snooze(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<SnoozeArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::snooze(&mut conn, args.issue_id, args.until).await?;
	Ok(Json(IssueData::from(issue)))
}

pub async fn unsnooze(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<IssueIdArgs>,
) -> Result<Json<IssueData>> {
	let mut conn = state.db.get().await?;
	let issue = Issue::unsnooze(&mut conn, args.issue_id).await?;
	Ok(Json(IssueData::from(issue)))
}
