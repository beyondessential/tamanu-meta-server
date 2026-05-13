//! Issues, events, incidents.
//!
//! See `docs/plans/issues-events-incidents.md` for the design rationale.

use commons_errors::{AppError, Result};
use commons_types::issue::Severity;
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{devices::Device, servers::Server};

#[derive(
	Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations, utoipa::ToSchema,
)]
#[diesel(belongs_to(Server))]
#[diesel(belongs_to(Device))]
#[diesel(table_name = crate::schema::issues)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Issue {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	pub server_id: Uuid,
	pub device_id: Option<Uuid>,
	pub source: String,
	#[diesel(column_name = "ref_")]
	#[serde(rename = "ref")]
	pub r#ref: String,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub severity: Severity,
	pub description: Option<String>,
	pub message: String,
	pub active: bool,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub first_seen: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub last_seen: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub acknowledged_at: Option<Timestamp>,
	pub acknowledged_by: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	/// Stored as nullable text; validated as `ResolvedReason` at the API layer
	/// (avoids the diesel orphan-rules dance for nullable enum columns).
	pub resolved_reason: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub snoozed_until: Option<Timestamp>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(Issue))]
#[diesel(table_name = crate::schema::events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Event {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub occurred_at: Option<Timestamp>,
	pub issue_id: Uuid,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub severity: Severity,
	pub description: Option<String>,
	pub message: String,
	pub active: bool,
	pub hash: Vec<u8>,
	pub occurrences: i32,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub last_seen: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(Server))]
#[diesel(table_name = crate::schema::incidents)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Incident {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	pub server_id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub opened_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub closed_at: Option<Timestamp>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub acknowledged_at: Option<Timestamp>,
	pub acknowledged_by: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolved_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(Incident))]
#[diesel(belongs_to(Issue))]
#[diesel(table_name = crate::schema::incident_issues)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct IncidentIssue {
	pub incident_id: Uuid,
	pub issue_id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub joined_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub left_at: Option<Timestamp>,
}

/// One event push from a device (public API) or operator (private API).
///
/// `ref` is required: clients that don't want dedup can mint a UUID.
/// `occurred_at` is optional and is the client's "when the thing happened"
/// timestamp; `created_at` is always server-set to NOW().
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewEvent {
	pub source: String,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IssueFilter {
	#[default]
	ActiveOnly,
	All,
}

/// Multi-field filter for the cross-server issues list (global view).
/// Each field is opt-in: `Default` matches everything currently active.
#[derive(Debug, Clone, Default)]
pub struct IssueListFilters {
	pub active_only: bool,
	pub severities: Option<Vec<Severity>>,
	/// Any server id in the group; the query walks to the root and then
	/// restricts to issues on servers in the root's descendant tree.
	pub server_group_id: Option<Uuid>,
	/// `Some(true)` = acknowledged; `Some(false)` = un-acknowledged; `None` = either.
	pub acked: Option<bool>,
}

fn hash_event(
	severity: Severity,
	active: bool,
	message: &str,
	description: Option<&str>,
) -> Vec<u8> {
	let mut h = Sha256::new();
	h.update(severity.to_string().as_bytes());
	h.update([0]);
	h.update([u8::from(active)]);
	h.update([0]);
	h.update(message.as_bytes());
	h.update([0]);
	h.update(description.unwrap_or("").as_bytes());
	h.finalize().to_vec()
}

impl NewEvent {
	/// Persist this event push:
	/// 1. find-or-create the issue keyed by (server_id, source, ref),
	/// 2. append the event or coalesce into the latest matching one,
	/// 3. (re)evaluate incident contribution.
	///
	/// `server_id` is the server the issue is attached to: derived from the
	/// device for public submissions, supplied by the operator for manual.
	/// `device_id` is `None` for manual events.
	pub async fn save(
		self,
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		device_id: Option<Uuid>,
	) -> Result<Issue> {
		use crate::schema::{events, issues};

		let severity = self.severity.unwrap_or_default();
		let active = self.active.unwrap_or(true);
		let now = Timestamp::now();
		let effective_time = self.occurred_at.unwrap_or(now);
		let description = self.description.as_deref();
		let hash = hash_event(severity, active, &self.message, description);

		// Find the root server for the group up-front; needed if we end up
		// opening an incident. A single recursive CTE walks parent_server_id.
		let root_server_id = Server::root_id(db, server_id).await?;

		db.transaction::<_, AppError, _>(async |conn| {
			// 1. find-or-create issue (FOR UPDATE so concurrent pushes on
			//    the same (server, source, ref) serialize cleanly).
			let existing: Option<Issue> = issues::table
				.select(Issue::as_select())
				.filter(
					issues::server_id
						.eq(server_id)
						.and(issues::source.eq(&self.source))
						.and(issues::ref_.eq(&self.r#ref)),
				)
				.for_update()
				.first(conn)
				.await
				.optional()?;

			let issue: Issue = if let Some(existing) = existing {
				let new_last_seen = if effective_time > existing.last_seen {
					effective_time
				} else {
					existing.last_seen
				};
				// Sentry-style reopen: a device event with `active = true` against a
				// human-resolved issue clears the resolved_* fields (issue is back
				// in unresolved state). Ack is *not* cleared — the operator's "I
				// know about this" still applies through a reopen.
				let clear_resolved = active && existing.resolved_at.is_some();
				let issue = diesel::update(issues::table.filter(issues::id.eq(existing.id)))
					.set((
						issues::device_id.eq(device_id),
						issues::severity.eq(severity),
						issues::description.eq(description),
						issues::message.eq(&self.message),
						issues::active.eq(active),
						issues::last_seen.eq(jiff_diesel::Timestamp::from(new_last_seen)),
						issues::resolved_at.eq(diesel::dsl::sql::<
							diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>,
						>(if clear_resolved {
							"NULL"
						} else {
							"issues.resolved_at"
						})),
						issues::resolved_by.eq(diesel::dsl::sql::<
							diesel::sql_types::Nullable<diesel::sql_types::Text>,
						>(if clear_resolved {
							"NULL"
						} else {
							"issues.resolved_by"
						})),
						issues::resolved_reason.eq(diesel::dsl::sql::<
							diesel::sql_types::Nullable<diesel::sql_types::Text>,
						>(if clear_resolved {
							"NULL"
						} else {
							"issues.resolved_reason"
						})),
					))
					.returning(Issue::as_select())
					.get_result(conn)
					.await?;
				issue
			} else {
				diesel::insert_into(issues::table)
					.values((
						issues::server_id.eq(server_id),
						issues::device_id.eq(device_id),
						issues::source.eq(&self.source),
						issues::ref_.eq(&self.r#ref),
						issues::severity.eq(severity),
						issues::description.eq(description),
						issues::message.eq(&self.message),
						issues::active.eq(active),
						issues::first_seen.eq(jiff_diesel::Timestamp::from(effective_time)),
						issues::last_seen.eq(jiff_diesel::Timestamp::from(effective_time)),
					))
					.returning(Issue::as_select())
					.get_result(conn)
					.await?
			};

			// 2. coalesce into latest event or insert new.
			let latest_event: Option<Event> = events::table
				.select(Event::as_select())
				.filter(events::issue_id.eq(issue.id))
				.order(events::created_at.desc())
				.first(conn)
				.await
				.optional()?;

			let coalesce = matches!(&latest_event, Some(e) if e.hash == hash);
			if let (true, Some(latest)) = (coalesce, latest_event) {
				let new_last = if effective_time > latest.last_seen {
					effective_time
				} else {
					latest.last_seen
				};
				diesel::update(events::table.filter(events::id.eq(latest.id)))
					.set((
						events::occurrences.eq(latest.occurrences + 1),
						events::last_seen.eq(jiff_diesel::Timestamp::from(new_last)),
					))
					.execute(conn)
					.await?;
			} else {
				diesel::insert_into(events::table)
					.values((
						events::issue_id.eq(issue.id),
						events::occurred_at.eq(self.occurred_at.map(jiff_diesel::Timestamp::from)),
						events::severity.eq(severity),
						events::description.eq(description),
						events::message.eq(&self.message),
						events::active.eq(active),
						events::hash.eq(&hash),
						events::last_seen.eq(jiff_diesel::Timestamp::from(effective_time)),
					))
					.execute(conn)
					.await?;
			}

			// 3. (re-)evaluate incident contribution against the new issue state.
			re_evaluate_incident_membership(conn, &issue, root_server_id, effective_time).await?;

			Ok(issue)
		})
		.await
	}
}

/// Compute whether the issue *should* currently be contributing to an
/// open incident, and apply join/leave accordingly. The rules:
///
/// - **Leave**: `!active || resolved || snoozed`. Severity downgrade alone
///   does *not* remove an issue — once contributing, it stays until it's
///   actually gone or explicitly suppressed.
/// - **Join**: not leaving, AND one of:
///   - severity ≥ floor (`error`), so this issue is high-priority enough to
///     create a new incident on its own; or
///   - the group already has an open incident — then any active issue,
///     even low-severity ones, joins it. The threshold only governs
///     incident *creation*; once an incident is in progress everything else
///     piles in for context.
async fn re_evaluate_incident_membership(
	conn: &mut AsyncPgConnection,
	issue: &Issue,
	root_server_id: Uuid,
	transition_time: Timestamp,
) -> Result<()> {
	use crate::schema::{incident_issues, incidents};

	let was_in = is_issue_in_open_incident(conn, issue.id).await?;
	let snoozed = issue.snoozed_until.map_or(false, |t| t > Timestamp::now());
	let group_open = group_has_open_incident(conn, root_server_id).await?;

	let should_leave = !issue.active || issue.resolved_at.is_some() || snoozed;
	let should_join = issue.active
		&& issue.resolved_at.is_none()
		&& !snoozed
		&& (issue.severity.opens_incident() || group_open);

	match (was_in, should_join, should_leave) {
		(false, true, _) => {
			let (incident_id, newly_opened) =
				find_or_open_incident(conn, root_server_id, transition_time).await?;
			diesel::insert_into(incident_issues::table)
				.values((
					incident_issues::incident_id.eq(incident_id),
					incident_issues::issue_id.eq(issue.id),
					incident_issues::joined_at.eq(jiff_diesel::Timestamp::from(transition_time)),
				))
				.execute(conn)
				.await?;
			if newly_opened {
				enqueue_slack_open(conn, incident_id, root_server_id, issue).await?;
			}
		}
		(true, _, true) => {
			let open_link: IncidentIssue = incident_issues::table
				.select(IncidentIssue::as_select())
				.filter(
					incident_issues::issue_id
						.eq(issue.id)
						.and(incident_issues::left_at.is_null()),
				)
				.for_update()
				.first(conn)
				.await?;
			diesel::update(
				incident_issues::table.filter(
					incident_issues::incident_id
						.eq(open_link.incident_id)
						.and(incident_issues::issue_id.eq(open_link.issue_id))
						.and(
							incident_issues::joined_at
								.eq(jiff_diesel::Timestamp::from(open_link.joined_at)),
						),
				),
			)
			.set(incident_issues::left_at.eq(jiff_diesel::Timestamp::from(transition_time)))
			.execute(conn)
			.await?;

			let remaining_open: i64 = incident_issues::table
				.filter(
					incident_issues::incident_id
						.eq(open_link.incident_id)
						.and(incident_issues::left_at.is_null()),
				)
				.count()
				.get_result(conn)
				.await?;
			if remaining_open == 0 {
				let closed: Incident = diesel::update(
					incidents::table.filter(incidents::id.eq(open_link.incident_id)),
				)
				.set(incidents::closed_at.eq(jiff_diesel::Timestamp::from(transition_time)))
				.returning(Incident::as_select())
				.get_result(conn)
				.await?;
				enqueue_slack_cascade_close(conn, &closed).await?;
			}
		}
		_ => {}
	}
	Ok(())
}

async fn is_issue_in_open_incident(db: &mut AsyncPgConnection, issue_id: Uuid) -> Result<bool> {
	use crate::schema::incident_issues;

	let count: i64 = incident_issues::table
		.filter(
			incident_issues::issue_id
				.eq(issue_id)
				.and(incident_issues::left_at.is_null()),
		)
		.count()
		.get_result(db)
		.await?;
	Ok(count > 0)
}

async fn group_has_open_incident(db: &mut AsyncPgConnection, root_server_id: Uuid) -> Result<bool> {
	use crate::schema::incidents::dsl;

	let count: i64 = dsl::incidents
		.filter(dsl::server_id.eq(root_server_id))
		.filter(dsl::closed_at.is_null())
		.count()
		.get_result(db)
		.await?;
	Ok(count > 0)
}

/// Returns the open incident's id and whether this call newly opened it.
/// The boolean is consumed by `re_evaluate_incident_membership` to decide
/// whether a Slack `incident_open` outbox row should be enqueued (re-joining
/// an existing incident shouldn't re-notify).
async fn find_or_open_incident(
	db: &mut AsyncPgConnection,
	root_server_id: Uuid,
	opened_at: Timestamp,
) -> Result<(Uuid, bool)> {
	use crate::schema::incidents;

	let open: Option<Incident> = incidents::table
		.select(Incident::as_select())
		.filter(
			incidents::server_id
				.eq(root_server_id)
				.and(incidents::closed_at.is_null()),
		)
		.order(incidents::opened_at.desc())
		.first(db)
		.await
		.optional()?;
	if let Some(inc) = open {
		return Ok((inc.id, false));
	}

	let new_incident: Incident = diesel::insert_into(incidents::table)
		.values((
			incidents::server_id.eq(root_server_id),
			incidents::opened_at.eq(jiff_diesel::Timestamp::from(opened_at)),
		))
		.returning(Incident::as_select())
		.get_result(db)
		.await?;
	Ok((new_incident.id, true))
}

async fn enqueue_slack_open(
	conn: &mut AsyncPgConnection,
	incident_id: Uuid,
	root_server_id: Uuid,
	issue: &Issue,
) -> Result<()> {
	let server = Server::get_by_id(conn, root_server_id).await?;
	let payload = crate::slack_outbox::vars::incident_open(
		&server,
		issue.severity,
		&issue.source,
		&issue.r#ref,
		&issue.message,
	);
	crate::slack_outbox::SlackOutbox::enqueue(
		conn,
		crate::slack_outbox::KIND_INCIDENT_OPEN,
		incident_id,
		Some(issue.id),
		None,
		payload,
	)
	.await?;
	Ok(())
}

async fn enqueue_slack_resolve(
	conn: &mut AsyncPgConnection,
	incident: &Incident,
	by: &str,
) -> Result<()> {
	enqueue_slack_resolve_inner(conn, incident, Some(by)).await
}

/// Cascade close: every issue left the incident, so we close it without an
/// operator. Posts a "resolved by automation" Slack notification so the
/// channel doesn't lose the close event.
async fn enqueue_slack_cascade_close(
	conn: &mut AsyncPgConnection,
	incident: &Incident,
) -> Result<()> {
	enqueue_slack_resolve_inner(conn, incident, None).await
}

async fn enqueue_slack_resolve_inner(
	conn: &mut AsyncPgConnection,
	incident: &Incident,
	by: Option<&str>,
) -> Result<()> {
	let server = Server::get_by_id(conn, incident.server_id).await?;
	let payload = crate::slack_outbox::vars::incident_resolve(&server, by);
	crate::slack_outbox::SlackOutbox::enqueue(
		conn,
		crate::slack_outbox::KIND_INCIDENT_RESOLVE,
		incident.id,
		None,
		None,
		payload,
	)
	.await?;
	Ok(())
}

impl Issue {
	pub async fn list_for_device(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
		filter: IssueFilter,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::issues::dsl;

		let mut q = dsl::issues
			.select(Self::as_select())
			.filter(dsl::device_id.eq(device_id))
			.into_boxed();
		if filter == IssueFilter::ActiveOnly {
			q = q
				.filter(dsl::active.eq(true))
				.filter(dsl::resolved_at.is_null());
		}
		q.order(dsl::last_seen.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_for_server(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		filter: IssueFilter,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::issues::dsl;

		let mut q = dsl::issues
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server_id))
			.into_boxed();
		if filter == IssueFilter::ActiveOnly {
			q = q
				.filter(dsl::active.eq(true))
				.filter(dsl::resolved_at.is_null());
		}
		q.order(dsl::last_seen.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Filtered cross-server issues list. Used by the global Incidents page.
	///
	/// - `active_only`: when true, only `active = true` *and* unresolved
	///   issues — operator-resolved items don't count even if the source
	///   keeps pushing them.
	/// - `severities`: when `Some` and non-empty, restrict to those.
	/// - `server_group_id`: when `Some`, restrict to issues whose server is
	///   in the descendant tree of that root (uses a recursive CTE via
	///   `Server::descendant_ids`).
	/// - `acked`: when `Some(true)`, only acknowledged; `Some(false)`, only
	///   un-acknowledged; `None`, either.
	pub async fn list(
		db: &mut AsyncPgConnection,
		filters: IssueListFilters,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::issues::dsl;

		// Resolve to the group root then enumerate descendants up-front
		// (two recursive CTEs); cheaper than embedding both into the main
		// query, and lets the later filter be a plain `IN`. Callers can
		// pass any server id in the group — the root walk handles it.
		let group_ids = if let Some(any_in_group) = filters.server_group_id {
			let root = Server::root_id(db, any_in_group).await?;
			Some(Server::descendant_ids(db, root).await?)
		} else {
			None
		};

		let mut q = dsl::issues.select(Self::as_select()).into_boxed();
		if filters.active_only {
			q = q
				.filter(dsl::active.eq(true))
				.filter(dsl::resolved_at.is_null());
		}
		if let Some(sevs) = filters.severities.as_ref().filter(|v| !v.is_empty()) {
			let strs: Vec<String> = sevs.iter().map(|s| s.to_string()).collect();
			q = q.filter(dsl::severity.eq_any(strs));
		}
		if let Some(ids) = group_ids {
			q = q.filter(dsl::server_id.eq_any(ids));
		}
		match filters.acked {
			Some(true) => q = q.filter(dsl::acknowledged_at.is_not_null()),
			Some(false) => q = q.filter(dsl::acknowledged_at.is_null()),
			None => {}
		}
		q.order(dsl::last_seen.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, issue_id: Uuid) -> Result<Self> {
		use crate::schema::issues::dsl;

		dsl::issues
			.select(Self::as_select())
			.filter(dsl::id.eq(issue_id))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Bulk lookup of issues that share the same `(source, ref)` across many
	/// servers. Each `(server_id, source, ref)` is unique, so at most one row
	/// per server is returned. Used by the canopy reachability sweep.
	pub async fn list_by_source_ref(
		db: &mut AsyncPgConnection,
		source: &str,
		ref_: &str,
		server_ids: &[Uuid],
	) -> Result<Vec<Self>> {
		use crate::schema::issues::dsl;
		if server_ids.is_empty() {
			return Ok(Vec::new());
		}
		dsl::issues
			.select(Self::as_select())
			.filter(
				dsl::source
					.eq(source)
					.and(dsl::ref_.eq(ref_))
					.and(dsl::server_id.eq_any(server_ids)),
			)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Mark an issue as acknowledged (or update the acker). Doesn't touch
	/// incident membership — ack is purely informational.
	pub async fn ack(db: &mut AsyncPgConnection, issue_id: Uuid, by: &str) -> Result<Self> {
		use crate::schema::issues;

		diesel::update(issues::table.filter(issues::id.eq(issue_id)))
			.set((
				issues::acknowledged_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				issues::acknowledged_by.eq(Some(by)),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn unack(db: &mut AsyncPgConnection, issue_id: Uuid) -> Result<Self> {
		use crate::schema::issues;
		diesel::update(issues::table.filter(issues::id.eq(issue_id)))
			.set((
				issues::acknowledged_at.eq(None::<jiff_diesel::Timestamp>),
				issues::acknowledged_by.eq(None::<String>),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Mark an issue as human-resolved. Triggers incident-membership
	/// re-evaluation (typically: leaves the incident).
	pub async fn resolve(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		by: &str,
		reason: commons_types::issue::ResolvedReason,
	) -> Result<Self> {
		use crate::schema::issues;
		let now = Timestamp::now();

		db.transaction::<_, AppError, _>(async |conn| {
			let issue = diesel::update(issues::table.filter(issues::id.eq(issue_id)))
				.set((
					issues::resolved_at.eq(jiff_diesel::Timestamp::from(now)),
					issues::resolved_by.eq(Some(by)),
					issues::resolved_reason.eq(Some(reason.to_string())),
				))
				.returning(Self::as_select())
				.get_result(conn)
				.await?;
			let root = Server::root_id(conn, issue.server_id).await?;
			re_evaluate_incident_membership(conn, &issue, root, now).await?;
			Ok(issue)
		})
		.await
	}

	pub async fn unresolve(db: &mut AsyncPgConnection, issue_id: Uuid) -> Result<Self> {
		use crate::schema::issues;
		let now = Timestamp::now();

		db.transaction::<_, AppError, _>(async |conn| {
			let issue = diesel::update(issues::table.filter(issues::id.eq(issue_id)))
				.set((
					issues::resolved_at.eq(None::<jiff_diesel::Timestamp>),
					issues::resolved_by.eq(None::<String>),
					issues::resolved_reason.eq(None::<String>),
				))
				.returning(Self::as_select())
				.get_result(conn)
				.await?;
			let root = Server::root_id(conn, issue.server_id).await?;
			re_evaluate_incident_membership(conn, &issue, root, now).await?;
			Ok(issue)
		})
		.await
	}

	/// Snooze an issue until the given timestamp. While snoozed, the issue
	/// can't open or join incidents. Triggers re-evaluation.
	pub async fn snooze(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		until: Timestamp,
	) -> Result<Self> {
		use crate::schema::issues;
		let now = Timestamp::now();

		db.transaction::<_, AppError, _>(async |conn| {
			let issue = diesel::update(issues::table.filter(issues::id.eq(issue_id)))
				.set(issues::snoozed_until.eq(jiff_diesel::Timestamp::from(until)))
				.returning(Self::as_select())
				.get_result(conn)
				.await?;
			let root = Server::root_id(conn, issue.server_id).await?;
			re_evaluate_incident_membership(conn, &issue, root, now).await?;
			Ok(issue)
		})
		.await
	}

	pub async fn unsnooze(db: &mut AsyncPgConnection, issue_id: Uuid) -> Result<Self> {
		use crate::schema::issues;
		let now = Timestamp::now();

		db.transaction::<_, AppError, _>(async |conn| {
			let issue = diesel::update(issues::table.filter(issues::id.eq(issue_id)))
				.set(issues::snoozed_until.eq(None::<jiff_diesel::Timestamp>))
				.returning(Self::as_select())
				.get_result(conn)
				.await?;
			let root = Server::root_id(conn, issue.server_id).await?;
			re_evaluate_incident_membership(conn, &issue, root, now).await?;
			Ok(issue)
		})
		.await
	}
}

/// Minimal incident metadata returned alongside an issue so the UI can
/// link from the issue card to the incident(s) the issue is attached to.
#[derive(Debug, Clone)]
pub struct IssueIncidentRef {
	pub incident_id: Uuid,
	pub opened_at: Timestamp,
	pub closed_at: Option<Timestamp>,
}

/// Aggregate counts displayed against an incident in the UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct IncidentStats {
	pub issue_count: i64,
	pub event_count: i64,
	/// `incident_notes` for this incident + `issue_notes` across all linked issues.
	pub note_count: i64,
}

impl Incident {
	/// Bulk-fetch stats for a set of incidents in four grouped queries —
	/// issues, events, incident-notes, issue-notes — run concurrently on
	/// four pool connections. Missing incident_ids get `Default` (zero).
	///
	/// Takes the pool (`&Db`) rather than a single connection so the four
	/// futures don't fight over one mutable handle.
	pub async fn stats_for(
		pool: &crate::Db,
		incident_ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, IncidentStats>> {
		use crate::schema::{events, incident_issues, incident_notes, issue_notes};
		use diesel::dsl::count_star;
		use std::collections::HashMap;

		let mut out: HashMap<Uuid, IncidentStats> = incident_ids
			.iter()
			.map(|id| (*id, IncidentStats::default()))
			.collect();
		if incident_ids.is_empty() {
			return Ok(out);
		}

		// Each future grabs its own pool connection so the four queries
		// run in parallel rather than serialised on one mutable conn.
		let ids = incident_ids.to_vec();
		let f_issues = async {
			let mut c = pool.get().await?;
			incident_issues::table
				.group_by(incident_issues::incident_id)
				.select((incident_issues::incident_id, count_star()))
				.filter(incident_issues::incident_id.eq_any(&ids))
				.load::<(Uuid, i64)>(&mut c)
				.await
				.map_err(AppError::from)
		};
		let f_events = async {
			let mut c = pool.get().await?;
			events::table
				.inner_join(
					incident_issues::table.on(events::issue_id.eq(incident_issues::issue_id)),
				)
				.group_by(incident_issues::incident_id)
				.select((incident_issues::incident_id, count_star()))
				.filter(incident_issues::incident_id.eq_any(&ids))
				.load::<(Uuid, i64)>(&mut c)
				.await
				.map_err(AppError::from)
		};
		let f_inotes = async {
			let mut c = pool.get().await?;
			incident_notes::table
				.group_by(incident_notes::incident_id)
				.select((incident_notes::incident_id, count_star()))
				.filter(incident_notes::incident_id.eq_any(&ids))
				.load::<(Uuid, i64)>(&mut c)
				.await
				.map_err(AppError::from)
		};
		let f_jnotes = async {
			let mut c = pool.get().await?;
			issue_notes::table
				.inner_join(
					incident_issues::table.on(issue_notes::issue_id.eq(incident_issues::issue_id)),
				)
				.group_by(incident_issues::incident_id)
				.select((incident_issues::incident_id, count_star()))
				.filter(incident_issues::incident_id.eq_any(&ids))
				.load::<(Uuid, i64)>(&mut c)
				.await
				.map_err(AppError::from)
		};
		let (issue_rows, event_rows, inote_rows, jnote_rows) =
			futures::try_join!(f_issues, f_events, f_inotes, f_jnotes)?;

		for (id, n) in issue_rows {
			out.entry(id).or_default().issue_count = n;
		}
		for (id, n) in event_rows {
			out.entry(id).or_default().event_count = n;
		}
		for (id, n) in inote_rows {
			out.entry(id).or_default().note_count += n;
		}
		for (id, n) in jnote_rows {
			out.entry(id).or_default().note_count += n;
		}

		Ok(out)
	}
}

impl Event {
	pub async fn list_for_issue(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::events::dsl;

		dsl::events
			.select(Self::as_select())
			.filter(dsl::issue_id.eq(issue_id))
			.order(dsl::created_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}
}

impl Incident {
	/// Incidents are owned by the *root* of the server group, so a caller
	/// asking for a child's incidents really wants the root's. Walk up the
	/// `parent_server_id` chain (same helper used at incident-open time) so
	/// the API returns the group's incidents regardless of which server in
	/// the group the caller named.
	pub async fn list_for_server(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		include_closed: bool,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::incidents::dsl;

		let root = Server::root_id(db, server_id).await?;
		let mut q = dsl::incidents
			.select(Self::as_select())
			.filter(dsl::server_id.eq(root))
			.into_boxed();
		if !include_closed {
			q = q.filter(dsl::closed_at.is_null());
		}
		q.order(dsl::opened_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_active(db: &mut AsyncPgConnection, limit: i64) -> Result<Vec<Self>> {
		use crate::schema::incidents::dsl;

		dsl::incidents
			.select(Self::as_select())
			.filter(dsl::closed_at.is_null())
			.order(dsl::opened_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Bulk lookup: for each issue id, the distinct incidents it is or was
	/// linked to, with their open/close timestamps. Used to surface the
	/// "attaching incident(s)" pill on each issue card.
	///
	/// The same `(issue, incident)` pair can have multiple `incident_issues`
	/// rows (issue left and rejoined). We dedupe on `incident_id` and order
	/// by `opened_at desc` so the most recent attachment is first.
	pub async fn for_issues(
		db: &mut AsyncPgConnection,
		issue_ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, Vec<IssueIncidentRef>>> {
		use crate::schema::{incident_issues, incidents};
		use std::collections::HashMap;

		let mut out: HashMap<Uuid, Vec<IssueIncidentRef>> = HashMap::new();
		if issue_ids.is_empty() {
			return Ok(out);
		}

		let rows: Vec<(
			Uuid,
			Uuid,
			jiff_diesel::Timestamp,
			jiff_diesel::NullableTimestamp,
		)> = incident_issues::table
			.inner_join(incidents::table.on(incidents::id.eq(incident_issues::incident_id)))
			.filter(incident_issues::issue_id.eq_any(issue_ids))
			.select((
				incident_issues::issue_id,
				incidents::id,
				incidents::opened_at,
				incidents::closed_at,
			))
			.distinct()
			.order(incidents::opened_at.desc())
			.load(db)
			.await?;

		for (issue_id, incident_id, opened_at, closed_at) in rows {
			out.entry(issue_id).or_default().push(IssueIncidentRef {
				incident_id,
				opened_at: opened_at.into(),
				closed_at: Option::<Timestamp>::from(closed_at),
			});
		}
		Ok(out)
	}

	pub async fn get_with_issues(
		db: &mut AsyncPgConnection,
		incident_id: Uuid,
	) -> Result<(Self, Vec<(IncidentIssue, Issue)>)> {
		use crate::schema::{incident_issues, incidents, issues};

		let incident: Self = incidents::table
			.select(Self::as_select())
			.filter(incidents::id.eq(incident_id))
			.first(db)
			.await?;

		let rows: Vec<(IncidentIssue, Issue)> = incident_issues::table
			.inner_join(issues::table.on(issues::id.eq(incident_issues::issue_id)))
			.select((IncidentIssue::as_select(), Issue::as_select()))
			.filter(incident_issues::incident_id.eq(incident_id))
			.order(incident_issues::joined_at.asc())
			.load(db)
			.await?;
		Ok((incident, rows))
	}

	pub async fn ack(db: &mut AsyncPgConnection, incident_id: Uuid, by: &str) -> Result<Self> {
		use crate::schema::incidents;

		diesel::update(incidents::table.filter(incidents::id.eq(incident_id)))
			.set((
				incidents::acknowledged_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				incidents::acknowledged_by.eq(Some(by)),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn unack(db: &mut AsyncPgConnection, incident_id: Uuid) -> Result<Self> {
		use crate::schema::incidents;
		diesel::update(incidents::table.filter(incidents::id.eq(incident_id)))
			.set((
				incidents::acknowledged_at.eq(None::<jiff_diesel::Timestamp>),
				incidents::acknowledged_by.eq(None::<String>),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Mark an incident as human-resolved. This is metadata only — it does
	/// *not* force `closed_at` (that's still driven by auto rules). An
	/// open incident can be resolved (acknowledging the cause) and a closed
	/// incident can be resolved retroactively.
	/// Resolve the incident. Cascades to every currently-contributing issue
	/// (active link), marking them with the same reason and timestamp; each
	/// one triggers `re_evaluate_incident_membership`, so the normal
	/// auto-close path sets `closed_at` once the last issue leaves. As a
	/// belt-and-suspenders, force-closes the incident if the cascade left
	/// it open (e.g. an incident with no live links — shouldn't happen, but
	/// we'd rather not leave the UI in a 'resolved but still open' state).
	pub async fn resolve(
		db: &mut AsyncPgConnection,
		incident_id: Uuid,
		by: &str,
		reason: commons_types::issue::ResolvedReason,
	) -> Result<Self> {
		use crate::schema::{incident_issues, incidents, issues};
		let now = Timestamp::now();

		db.transaction::<_, AppError, _>(async |conn| {
			let open_issue_ids: Vec<Uuid> = incident_issues::table
				.select(incident_issues::issue_id)
				.filter(
					incident_issues::incident_id
						.eq(incident_id)
						.and(incident_issues::left_at.is_null()),
				)
				.load(conn)
				.await?;

			for issue_id in open_issue_ids {
				let existing: Issue = issues::table
					.select(Issue::as_select())
					.filter(issues::id.eq(issue_id))
					.first(conn)
					.await?;
				if existing.resolved_at.is_some() {
					// Already resolved by the operator earlier; leave the
					// reason/by intact so the audit trail tells the truth.
					continue;
				}
				let issue = diesel::update(issues::table.filter(issues::id.eq(issue_id)))
					.set((
						issues::resolved_at.eq(jiff_diesel::Timestamp::from(now)),
						issues::resolved_by.eq(Some(by)),
						issues::resolved_reason.eq(Some(reason.to_string())),
					))
					.returning(Issue::as_select())
					.get_result(conn)
					.await?;
				let root = Server::root_id(conn, issue.server_id).await?;
				re_evaluate_incident_membership(conn, &issue, root, now).await?;
			}

			let incident: Incident =
				diesel::update(incidents::table.filter(incidents::id.eq(incident_id)))
					.set((
						incidents::resolved_at.eq(jiff_diesel::Timestamp::from(now)),
						incidents::resolved_by.eq(Some(by)),
						incidents::resolved_reason.eq(Some(reason.to_string())),
					))
					.returning(Incident::as_select())
					.get_result(conn)
					.await?;

			if incident.closed_at.is_some() {
				return Ok(incident);
			}
			let incident: Incident =
				diesel::update(incidents::table.filter(incidents::id.eq(incident_id)))
					.set(incidents::closed_at.eq(jiff_diesel::Timestamp::from(now)))
					.returning(Incident::as_select())
					.get_result(conn)
					.await?;
			enqueue_slack_resolve(conn, &incident, by).await?;
			Ok(incident)
		})
		.await
	}

	pub async fn unresolve(db: &mut AsyncPgConnection, incident_id: Uuid) -> Result<Self> {
		use crate::schema::incidents;
		diesel::update(incidents::table.filter(incidents::id.eq(incident_id)))
			.set((
				incidents::resolved_at.eq(None::<jiff_diesel::Timestamp>),
				incidents::resolved_by.eq(None::<String>),
				incidents::resolved_reason.eq(None::<String>),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}
}
