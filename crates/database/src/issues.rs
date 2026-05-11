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

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
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
#[derive(Debug, Clone, Deserialize)]
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

fn hash_event(severity: Severity, active: bool, message: &str, description: Option<&str>) -> Vec<u8> {
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
		let root_server_id = root_server_id(db, server_id).await?;

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
						issues::resolved_at.eq(diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>>(
							if clear_resolved { "NULL" } else { "issues.resolved_at" },
						)),
						issues::resolved_by.eq(diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Text>>(
							if clear_resolved { "NULL" } else { "issues.resolved_by" },
						)),
						issues::resolved_reason.eq(diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Text>>(
							if clear_resolved { "NULL" } else { "issues.resolved_reason" },
						)),
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
						events::occurred_at
							.eq(self.occurred_at.map(jiff_diesel::Timestamp::from)),
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
/// - **Should contribute** iff `active && severity >= floor && !resolved &&
///   !snoozed`.
/// - **Join**: insert a fresh `incident_issues` row (creating an incident if
///   the group has no open one).
/// - **Leave**: set `left_at` on the open link row; if it was the last open
///   contributor for that incident, set incident `closed_at`.
///
/// Once an issue is contributing, severity downgrades do *not* remove it —
/// only one of (active flips off, human resolve, snooze) closes the link.
async fn re_evaluate_incident_membership(
	conn: &mut AsyncPgConnection,
	issue: &Issue,
	root_server_id: Uuid,
	transition_time: Timestamp,
) -> Result<()> {
	use crate::schema::{incident_issues, incidents};

	let was_in = is_issue_in_open_incident(conn, issue.id).await?;
	let snoozed = issue
		.snoozed_until
		.map_or(false, |t| t > Timestamp::now());
	let should_be_in = issue.active
		&& issue.severity.opens_incident()
		&& issue.resolved_at.is_none()
		&& !snoozed;

	match (was_in, should_be_in) {
		(false, true) => {
			let incident_id =
				find_or_open_incident(conn, root_server_id, transition_time).await?;
			diesel::insert_into(incident_issues::table)
				.values((
					incident_issues::incident_id.eq(incident_id),
					incident_issues::issue_id.eq(issue.id),
					incident_issues::joined_at
						.eq(jiff_diesel::Timestamp::from(transition_time)),
				))
				.execute(conn)
				.await?;
		}
		(true, false) => {
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
				diesel::update(
					incidents::table.filter(incidents::id.eq(open_link.incident_id)),
				)
				.set(incidents::closed_at.eq(jiff_diesel::Timestamp::from(transition_time)))
				.execute(conn)
				.await?;
			}
		}
		_ => {}
	}
	Ok(())
}

async fn root_server_id(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Uuid> {
	use diesel::sql_types::Uuid as SqlUuid;

	#[derive(QueryableByName)]
	struct RootId {
		#[diesel(sql_type = SqlUuid)]
		id: Uuid,
	}

	let row: RootId = diesel::sql_query(
		"WITH RECURSIVE chain AS (\
			SELECT id, parent_server_id FROM servers WHERE id = $1 \
			UNION ALL \
			SELECT s.id, s.parent_server_id FROM servers s \
				JOIN chain c ON s.id = c.parent_server_id \
		) SELECT id FROM chain WHERE parent_server_id IS NULL LIMIT 1",
	)
	.bind::<SqlUuid, _>(server_id)
	.get_result(db)
	.await?;
	Ok(row.id)
}

async fn is_issue_in_open_incident(
	db: &mut AsyncPgConnection,
	issue_id: Uuid,
) -> Result<bool> {
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

async fn find_or_open_incident(
	db: &mut AsyncPgConnection,
	root_server_id: Uuid,
	opened_at: Timestamp,
) -> Result<Uuid> {
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
		return Ok(inc.id);
	}

	let new_incident: Incident = diesel::insert_into(incidents::table)
		.values((
			incidents::server_id.eq(root_server_id),
			incidents::opened_at.eq(jiff_diesel::Timestamp::from(opened_at)),
		))
		.returning(Incident::as_select())
		.get_result(db)
		.await?;
	Ok(new_incident.id)
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
			q = q.filter(dsl::active.eq(true));
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
			q = q.filter(dsl::active.eq(true));
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

	/// Mark an issue as acknowledged (or update the acker). Doesn't touch
	/// incident membership — ack is purely informational.
	pub async fn ack(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		by: &str,
	) -> Result<Self> {
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
				issues::acknowledged_at
					.eq(None::<jiff_diesel::Timestamp>),
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
			let root = root_server_id(conn, issue.server_id).await?;
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
			let root = root_server_id(conn, issue.server_id).await?;
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
			let root = root_server_id(conn, issue.server_id).await?;
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
			let root = root_server_id(conn, issue.server_id).await?;
			re_evaluate_incident_membership(conn, &issue, root, now).await?;
			Ok(issue)
		})
		.await
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
	pub async fn list_for_server(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		include_closed: bool,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::incidents::dsl;

		let mut q = dsl::incidents
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server_id))
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

	pub async fn ack(
		db: &mut AsyncPgConnection,
		incident_id: Uuid,
		by: &str,
	) -> Result<Self> {
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
				incidents::acknowledged_at
					.eq(None::<jiff_diesel::Timestamp>),
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
	pub async fn resolve(
		db: &mut AsyncPgConnection,
		incident_id: Uuid,
		by: &str,
		reason: commons_types::issue::ResolvedReason,
	) -> Result<Self> {
		use crate::schema::incidents;

		diesel::update(incidents::table.filter(incidents::id.eq(incident_id)))
			.set((
				incidents::resolved_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				incidents::resolved_by.eq(Some(by)),
				incidents::resolved_reason.eq(Some(reason.to_string())),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
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
