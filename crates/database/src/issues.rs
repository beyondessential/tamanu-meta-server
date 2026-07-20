//! Issues, events, incidents.

use commons_errors::{AppError, Result};
use commons_types::status::CheckResult;
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{devices::Device, server_groups::ServerGroup, servers::Server};

/// A tracked problem or condition on a server, or on a server group as a
/// whole. An issue is opened the first time its source reports it and stays
/// open (accumulating events) until the source reports it as no longer
/// active or an operator resolves it. Issues are the basic unit that drives
/// incidents, Slack notifications, and the fleet health view.
#[derive(
	Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations, utoipa::ToSchema,
)]
#[diesel(belongs_to(Server))]
#[diesel(belongs_to(Device))]
#[diesel(table_name = crate::schema::issues)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Issue {
	/// Unique identifier for this issue.
	pub id: Uuid,
	/// When this issue was first created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// When this issue was last modified.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// The server this issue is attached to. `None` for a group-scoped issue
	/// (see `server_group_id`) — exactly one of the two is always set.
	pub server_id: Option<Uuid>,
	/// The server group this issue is attached to, for a control-plane issue
	/// (e.g. backup corruption, a failed preflight check) that isn't tied to
	/// any single server. `None` for an ordinary server-scoped issue.
	/// Group-scoped issues are always considered even if an individual
	/// server in the group has monitoring turned off.
	pub server_group_id: Option<Uuid>,
	/// The device that reported this issue, if it was raised by a device
	/// push. `None` for issues raised by an operator or by the platform
	/// itself.
	pub device_id: Option<Uuid>,
	/// Identifies what raised this issue — a healthcheck, a backup pipeline,
	/// an operator, etc. Used together with `ref` to detect repeat reports
	/// of the same underlying problem.
	pub source: String,
	/// A caller-chosen identifier for this issue within its `source`. The
	/// same `(source, ref)` pair reported again updates this issue instead
	/// of creating a new one.
	#[diesel(column_name = "ref_")]
	#[serde(rename = "ref")]
	pub r#ref: String,
	/// A short, single-line title for the issue, shown as its headline in
	/// the UI and in Slack notifications. `None` if no title was given.
	pub description: Option<String>,
	/// The full body text describing the issue.
	pub message: String,
	/// Whether the condition behind this issue is still ongoing. Set to
	/// `false` when the source reports the condition has cleared.
	pub active: bool,
	/// When this issue was first reported.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub first_seen: Timestamp,
	/// When this issue was most recently reported or updated.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub last_seen: Timestamp,
	/// When an operator marked this issue resolved. `None` if it hasn't
	/// been resolved.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub resolved_at: Option<Timestamp>,
	/// The operator who resolved this issue. `None` if it hasn't been
	/// resolved.
	pub resolved_by: Option<String>,
	/// The reason given when the issue was resolved (for example: fixed,
	/// false positive, won't fix). `None` if it hasn't been resolved.
	pub resolved_reason: Option<String>,
	/// If set, this issue is snoozed until this time: it's temporarily
	/// excluded from incidents and notifications even while still active.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub snoozed_until: Option<Timestamp>,
	/// The check this issue tracks, for issues that are check state (the
	/// ref minus its namespace prefix). `None` for issues that predate the
	/// check-state model or that no filing has stamped yet.
	pub check_name: Option<String>,
	/// The result the source reported on the latest filing, before policy.
	#[diesel(deserialize_as = MaybeCheckResult, serialize_as = Option<String>)]
	pub observed_result: Option<CheckResult>,
	/// What policy made of the latest observed result. This is the result
	/// canopy acts on; the transitional `severity` is derived from it.
	#[diesel(deserialize_as = MaybeCheckResult, serialize_as = Option<String>)]
	pub effective_result: Option<CheckResult>,
	/// The check's own fields from the latest report, verbatim (minus the
	/// reserved keys), for display alongside the state.
	pub detail: Option<serde_json::Value>,
	/// When the current degradation streak began, for state that is
	/// currently degraded (effective warning/failed/broken). `None` while
	/// healthy; a recovery clears it and a later degradation starts a
	/// fresh streak.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub degraded_since: Option<Timestamp>,
	/// When this state last filed degraded. Never cleared: distinguishes
	/// a recovered issue (worth listing) from always-healthy check state.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub last_degraded_at: Option<Timestamp>,
	/// Whether this check's policy escalates: an effective failure
	/// notifies immediately, bypassing incident grace. Stamped from the
	/// catalog on every filing.
	pub escalates: bool,
}

impl Issue {
	/// Does this state open an incident on its own? An effective failure
	/// does; anything less joins an already-open incident but doesn't
	/// create (or hold open) one.
	pub fn opens_incident(&self) -> bool {
		self.effective_result == Some(CheckResult::Failed)
	}

	/// Is this state an escalating failure right now — one whose
	/// notification bypasses the incident grace period?
	pub fn escalates_now(&self) -> bool {
		self.opens_incident() && self.escalates
	}
}

/// Diesel helper: a nullable text column read as an optional
/// [`CheckResult`], treating unparseable text as `None` rather than
/// failing the whole row load.
pub struct MaybeCheckResult(Option<CheckResult>);

impl From<MaybeCheckResult> for Option<CheckResult> {
	fn from(v: MaybeCheckResult) -> Self {
		v.0
	}
}

impl
	diesel::deserialize::Queryable<
		diesel::sql_types::Nullable<diesel::sql_types::Text>,
		diesel::pg::Pg,
	> for MaybeCheckResult
{
	type Row = Option<String>;

	fn build(row: Option<String>) -> diesel::deserialize::Result<Self> {
		Ok(Self(row.and_then(|s| s.parse().ok())))
	}
}

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(ServerGroup, foreign_key = server_group_id))]
#[diesel(table_name = crate::schema::incidents)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Incident {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// The server group this incident targets, or `None` for a canopy-wide
	/// incident (aggregating canopy-wide issues — self-alerts).
	pub server_group_id: Option<Uuid>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub opened_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub closed_at: Option<Timestamp>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolved_reason: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub escalated_at: Option<Timestamp>,
	/// When the incident's last effective failure left and lingering began.
	/// `None` while a failure is live. A failure returning clears it (the
	/// same incident continues); otherwise the linger sweep closes the
	/// incident once this is older than the target's linger window,
	/// backdating `closed_at` to it. Not cleared by the close itself.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub closing_at: Option<Timestamp>,
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

/// A single occurrence to report against an issue, sent either by a device
/// or by an operator. If an issue with the same `source` and `ref` is
/// already open, this occurrence is folded into it (updating its state and
/// bumping its last-seen time); otherwise a new issue is created.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewEvent {
	/// Identifies what is reporting this event — a healthcheck, a backup
	/// pipeline, an operator, etc.
	pub source: String,
	/// A caller-chosen identifier for this specific problem within `source`,
	/// used to detect repeated reports of the same underlying condition.
	/// Required — mint a UUID if deduplication isn't needed.
	#[serde(rename = "ref")]
	pub r#ref: String,
	/// A short, single-line title for this event, shown as the issue's
	/// headline in the UI and as the subject of any Slack notification.
	/// Must not contain newlines. Use `message` for the full body text.
	#[serde(default)]
	pub description: Option<String>,
	/// The full body text for this event. Free-form, multi-line text is
	/// fine. If `description` is omitted, the UI falls back to using the
	/// first line of this field as the headline.
	pub message: String,
	/// Whether the condition this event describes is still ongoing.
	/// Defaults to `true`. Sending `false` marks the condition as cleared,
	/// which can close the issue's contribution to any open incident.
	#[serde(default)]
	pub active: Option<bool>,
	/// When the event actually happened, if known and different from when
	/// it was received. Defaults to the time the event was received.
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
	/// Restrict to issues whose latest effective result is one of these.
	pub results: Option<Vec<CheckResult>>,
	/// Restrict to issues whose server belongs to this group.
	pub server_group_id: Option<Uuid>,
	/// When `Some`, restrict to issues last seen at or after this time.
	pub since: Option<Timestamp>,
}

/// The check-state stamp accompanying a filing that is a check result:
/// which check, both sides of the policy transform, and the check's own
/// detail from the report. Filings that aren't check results (device
/// event pushes, producers not yet on the check-state model) carry no
/// stamp and leave the state columns null.
#[derive(Debug, Clone)]
pub struct CheckStateStamp {
	pub check: String,
	pub observed: CheckResult,
	pub effective: CheckResult,
	/// Whether the check's policy escalates (see [`Issue::escalates`]).
	pub escalates: bool,
	pub detail: Option<serde_json::Value>,
}

/// Write a filing's check-state stamp onto its issue row, maintaining the
/// degraded-streak timestamps: `degraded_since` holds while degraded
/// (starting a fresh streak on the healthy → degraded transition, using
/// `prior` — the row's value before this filing), clears on recovery;
/// `last_degraded_at` never clears. Returns the updated row.
async fn stamp_check_state(
	conn: &mut AsyncPgConnection,
	issue_id: Uuid,
	prior: Option<(Option<Timestamp>, Option<Timestamp>)>,
	stamp: &CheckStateStamp,
	at: Timestamp,
) -> Result<Issue> {
	use crate::schema::issues;

	let degraded = matches!(
		stamp.effective,
		CheckResult::Warning | CheckResult::Failed | CheckResult::Broken
	);
	let (prior_degraded_since, prior_last_degraded) = prior.unwrap_or((None, None));
	let degraded_since = if degraded {
		Some(jiff_diesel::Timestamp::from(
			prior_degraded_since.unwrap_or(at),
		))
	} else {
		None
	};
	let last_degraded_at = if degraded {
		Some(jiff_diesel::Timestamp::from(at))
	} else {
		prior_last_degraded.map(jiff_diesel::Timestamp::from)
	};
	let issue = diesel::update(issues::table.filter(issues::id.eq(issue_id)))
		.set((
			issues::check_name.eq(&stamp.check),
			issues::observed_result.eq(stamp.observed.to_string()),
			issues::effective_result.eq(stamp.effective.to_string()),
			issues::escalates.eq(stamp.escalates),
			issues::detail.eq(&stamp.detail),
			issues::degraded_since.eq(degraded_since),
			issues::last_degraded_at.eq(last_degraded_at),
		))
		.returning(Issue::as_select())
		.get_result(conn)
		.await
		.map_err(AppError::from)?;
	// Every stamped filing also feeds the state's stability record (from
	// the observed result, so policy never feeds back into it).
	crate::stability::record_observation(conn, issue_id, stamp.observed, at).await?;
	Ok(issue)
}

impl NewEvent {
	/// Persist this event push:
	/// 1. find-or-create the issue keyed by (server_id, source, ref),
	///    updating its state from this report,
	/// 2. if the server is in a group: (re)evaluate incident contribution.
	///
	/// `server_id` is the server the issue is attached to: derived from the
	/// device for public submissions, supplied by the operator for manual.
	/// `device_id` is `None` for manual events.
	///
	/// Issues from an **ungrouped** server are still recorded — the issue
	/// row goes in just like any other push — but the incident
	/// flow is skipped (`server_group_id` would have nowhere to point).
	/// When the operator later assigns the server to a group,
	/// [`Server::assign_to_group`] runs `reevaluate_open_issues_for_server`
	/// over the now-grouped issues so anything that should have opened an
	/// incident does so retroactively.
	pub async fn save(
		self,
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		device_id: Option<Uuid>,
	) -> Result<Issue> {
		self.save_with_state(db, server_id, device_id, None, false)
			.await
	}

	/// [`Self::save`], stamping the issue's check-state columns from a
	/// check result. Status ingestion passes the stamp; everything else
	/// files via [`Self::save`].
	pub async fn save_with_state(
		self,
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		device_id: Option<Uuid>,
		state: Option<&CheckStateStamp>,
		defer_incident_eval: bool,
	) -> Result<Issue> {
		use crate::schema::issues;

		// description is the single-line title; reject multi-line input
		// up front so the UI never has to fight with a multi-line
		// "headline".
		if let Some(d) = self.description.as_deref()
			&& d.contains('\n')
		{
			return Err(AppError::BadRequest(
				"description must be a single line (no newlines); use `message` for body text"
					.into(),
			));
		}

		let active = self.active.unwrap_or(true);
		let now = Timestamp::now();
		let effective_time = self.occurred_at.unwrap_or(now);
		let description = self.description.as_deref();

		// Look up the server's group up-front; needed by the incident-open
		// path. If `None`, we still record the issue/event but skip
		// incident logic — see `reevaluate_open_issues_for_server` for the
		// catch-up path. `monitored` gates the incident workflow the same
		// way: ungrouped *or* unmonitored → issue/event still recorded,
		// but no incident contribution.
		let server = Server::get_by_id(db, server_id).await?;
		let server_group_id = server.group_id;
		let monitored = server.is_monitored;

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
				// Sentry-style reopen: a device event with `active = true` against an
				// operator-resolved issue clears the resolved_* fields (issue is back
				// in unresolved state).
				let clear_resolved = active && existing.resolved_at.is_some();
				if let Some(stamp) = state {
					stamp_check_state(
						conn,
						existing.id,
						Some((existing.degraded_since, existing.last_degraded_at)),
						stamp,
						effective_time,
					)
					.await?;
				}
				let issue = diesel::update(issues::table.filter(issues::id.eq(existing.id)))
					.set((
						issues::device_id.eq(device_id),
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
				let inserted: Issue = diesel::insert_into(issues::table)
					.values((
						issues::server_id.eq(server_id),
						issues::device_id.eq(device_id),
						issues::source.eq(&self.source),
						issues::ref_.eq(&self.r#ref),
						issues::description.eq(description),
						issues::message.eq(&self.message),
						issues::active.eq(active),
						issues::first_seen.eq(jiff_diesel::Timestamp::from(effective_time)),
						issues::last_seen.eq(jiff_diesel::Timestamp::from(effective_time)),
					))
					.returning(Issue::as_select())
					.get_result(conn)
					.await?;
				match state {
					Some(stamp) => {
						stamp_check_state(conn, inserted.id, None, stamp, effective_time).await?
					}
					None => inserted,
				}
			};

			// 2. (re-)evaluate incident contribution against the new issue state.
			//    `by = None`: this came from a device push, not an operator action.
			//    Skipped when the server is ungrouped: incidents are group-keyed.
			//    Skipped when `defer_incident_eval`: the high-frequency device
			//    ingest path enqueues the server for the reeval worker instead
			//    (see `enqueue_incident_reeval`), keeping the per-group lock off
			//    the request path.
			if let Some(gid) = server_group_id
				&& !defer_incident_eval
			{
				re_evaluate_incident_membership(
					conn,
					&issue,
					IncidentTarget::Group(gid),
					monitored,
					effective_time,
					None,
				)
				.await?;
			}

			Ok(issue)
		})
		.await
	}
}

/// Open (or recover) a **group-scoped** issue, bypassing the per-server
/// `is_monitored` gate. This is the single entrypoint for control-plane
/// concerns that are not attributable to any one server — backup corruption,
/// upstream preflight failures, reconcile-missing, restore-verification.
///
/// Mirrors [`NewEvent::save`] but keys the issue on
/// `(server_group_id, source, ref)` instead of `(server_id, …)`:
/// 1. find-or-create the group-scoped issue (server_id = NULL),
///    updating its state from this report,
/// 2. run incident membership evaluation with `monitored = true` so the
///    incident opens/pages regardless of any member server's monitored flag.
///
/// Recovery is the same `(source, ref)` with `active = false` at a lower
/// severity, which lets the issue leave the incident and auto-close —
/// identical lifecycle to the per-server path.
///
/// `description` is an optional single-line headline (rejected if multi-line);
/// `message` is the body.
pub async fn raise_group_event(
	conn: &mut AsyncPgConnection,
	group_id: Uuid,
	r#ref: &str,
	description: Option<&str>,
	message: &str,
	active: bool,
) -> Result<Issue> {
	raise_group_event_with_state(conn, group_id, r#ref, description, message, active, None).await
}

/// [`raise_group_event`], stamping the issue's check-state columns.
pub async fn raise_group_event_with_state(
	conn: &mut AsyncPgConnection,
	group_id: Uuid,
	r#ref: &str,
	description: Option<&str>,
	message: &str,
	active: bool,
	state: Option<&CheckStateStamp>,
) -> Result<Issue> {
	use crate::schema::issues;

	if let Some(d) = description
		&& d.contains('\n')
	{
		return Err(AppError::BadRequest(
			"description must be a single line (no newlines); use `message` for body text".into(),
		));
	}

	let source = crate::statuses::CANOPY_SOURCE;
	let now = Timestamp::now();

	conn.transaction::<_, AppError, _>(async |conn| {
		// 1. find-or-create the group-scoped issue.
		let existing: Option<Issue> = issues::table
			.select(Issue::as_select())
			.filter(
				issues::server_group_id
					.eq(group_id)
					.and(issues::source.eq(source))
					.and(issues::ref_.eq(r#ref)),
			)
			.for_update()
			.first(conn)
			.await
			.optional()?;

		let prior_state = existing
			.as_ref()
			.map(|e| (e.degraded_since, e.last_degraded_at));
		let issue: Issue = if let Some(existing) = existing {
			let new_last_seen = if now > existing.last_seen {
				now
			} else {
				existing.last_seen
			};
			let clear_resolved = active && existing.resolved_at.is_some();
			diesel::update(issues::table.filter(issues::id.eq(existing.id)))
				.set((
					issues::description.eq(description),
					issues::message.eq(message),
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
				.await?
		} else {
			diesel::insert_into(issues::table)
				.values((
					issues::server_group_id.eq(group_id),
					issues::source.eq(source),
					issues::ref_.eq(r#ref),
					issues::description.eq(description),
					issues::message.eq(message),
					issues::active.eq(active),
					issues::first_seen.eq(jiff_diesel::Timestamp::from(now)),
					issues::last_seen.eq(jiff_diesel::Timestamp::from(now)),
				))
				.returning(Issue::as_select())
				.get_result(conn)
				.await?
		};

		let issue = match state {
			Some(stamp) => stamp_check_state(conn, issue.id, prior_state, stamp, now).await?,
			None => issue,
		};

		// 2. group-aware incident evaluation — monitored = true unconditionally.
		re_evaluate_incident_membership(
			conn,
			&issue,
			IncidentTarget::Group(group_id),
			true,
			now,
			None,
		)
		.await?;

		Ok(issue)
	})
	.await
}

/// Open (or recover) a **canopy-wide** issue: one scoped to neither a
/// server nor a group, keyed `(source = canopy, ref)` under the global
/// partial unique index. This is the state store for canopy monitoring
/// its own operation (self-alerts).
///
/// Mirrors [`raise_group_event`]'s find-or-create; incident evaluation
/// runs against the global target, so canopy-wide issues get the full
/// incident lifecycle (grace, escalation, resolution) like any other.
pub async fn raise_global_event(
	conn: &mut AsyncPgConnection,
	r#ref: &str,
	description: Option<&str>,
	message: &str,
	active: bool,
) -> Result<Issue> {
	raise_global_event_with_state(conn, r#ref, description, message, active, None).await
}

/// [`raise_global_event`], stamping the issue's check-state columns.
pub async fn raise_global_event_with_state(
	conn: &mut AsyncPgConnection,
	r#ref: &str,
	description: Option<&str>,
	message: &str,
	active: bool,
	state: Option<&CheckStateStamp>,
) -> Result<Issue> {
	use crate::schema::issues;

	if let Some(d) = description
		&& d.contains('\n')
	{
		return Err(AppError::BadRequest(
			"description must be a single line (no newlines); use `message` for body text".into(),
		));
	}

	let source = crate::statuses::CANOPY_SOURCE;
	let now = Timestamp::now();

	conn.transaction::<_, AppError, _>(async |conn| {
		let existing: Option<Issue> = issues::table
			.select(Issue::as_select())
			.filter(
				issues::server_id
					.is_null()
					.and(issues::server_group_id.is_null())
					.and(issues::source.eq(source))
					.and(issues::ref_.eq(r#ref)),
			)
			.for_update()
			.first(conn)
			.await
			.optional()?;

		let prior_state = existing
			.as_ref()
			.map(|e| (e.degraded_since, e.last_degraded_at));
		let issue: Issue = if let Some(existing) = existing {
			let new_last_seen = if now > existing.last_seen {
				now
			} else {
				existing.last_seen
			};
			let clear_resolved = active && existing.resolved_at.is_some();
			diesel::update(issues::table.filter(issues::id.eq(existing.id)))
				.set((
					issues::description.eq(description),
					issues::message.eq(message),
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
				.await?
		} else {
			diesel::insert_into(issues::table)
				.values((
					issues::source.eq(source),
					issues::ref_.eq(r#ref),
					issues::description.eq(description),
					issues::message.eq(message),
					issues::active.eq(active),
					issues::first_seen.eq(jiff_diesel::Timestamp::from(now)),
					issues::last_seen.eq(jiff_diesel::Timestamp::from(now)),
				))
				.returning(Issue::as_select())
				.get_result(conn)
				.await?
		};

		let issue = match state {
			Some(stamp) => stamp_check_state(conn, issue.id, prior_state, stamp, now).await?,
			None => issue,
		};

		// Global incident evaluation — always monitored.
		re_evaluate_incident_membership(conn, &issue, IncidentTarget::Global, true, now, None)
			.await?;

		Ok(issue)
	})
	.await
}

/// Where a canopy-determined check's state attaches.
#[derive(Debug, Clone, Copy)]
pub enum FilingScope {
	Server {
		server_id: Uuid,
		device_id: Option<Uuid>,
	},
	Group(Uuid),
	Global,
}

/// The source operator-raised manual conditions file under.
pub const MANUAL_SOURCE: &str = "manual";

/// One canopy-determined or operator-raised check result to file:
/// reachability, backup health, key expiry, self-monitoring, manual
/// conditions, and the like.
#[derive(Debug, Clone)]
pub struct CheckFiling<'a> {
	/// The reserved source this filing belongs to: [`MANUAL_SOURCE`] for
	/// operator-raised conditions (server scope only), `canopy` for
	/// canopy's own determinations.
	pub source: &'a str,
	pub scope: FilingScope,
	/// The check's stable name (doubles as the issue ref under the
	/// source): a contract with stored silences.
	pub check: &'a str,
	/// What was observed this pass. Policy grades it from there.
	pub observed: CheckResult,
	/// Single-line headline for degraded filings.
	pub title: Option<&'a str>,
	pub message: &'a str,
	/// The check's own fields, available to policy rules as `check.*`
	/// and displayed alongside the state.
	pub detail: Option<serde_json::Value>,
	/// The policy this check registers with on first sight — the ceiling
	/// and escalation its condition warrants. Operator edits stick;
	/// these only seed the catalog row.
	pub default_ceiling: CheckResult,
	pub default_escalates: bool,
	/// The documentation canopy's own checks ship with, seeded into the
	/// catalog on first sight (never overwriting operator edits). See
	/// the CHK spec's Documentation section for the convention.
	pub documentation: Option<&'a str>,
}

/// File one canopy-determined or operator-raised check result: register
/// its catalog entry (first sight only), grade the observation through
/// the operator's policy, and upsert the check state at the right scope
/// — driving incident membership exactly like a device-reported check.
///
/// Group- and canopy-wide scopes are canopy's own (their raise paths
/// file under the `canopy` source); manual conditions are server-scoped.
///
/// Until issues themselves carry results, the effective result maps to
/// the issue severity the same way status ingestion does: failed →
/// error (critical when the policy escalates), warning/broken →
/// warning; passed and skipped record healthy state and close.
pub async fn file_check(conn: &mut AsyncPgConnection, filing: CheckFiling<'_>) -> Result<Issue> {
	use crate::check_policies::{CheckPolicy, EvaluationContext};

	let source = filing.source;
	debug_assert!(
		matches!(filing.scope, FilingScope::Server { .. })
			|| source == crate::statuses::CANOPY_SOURCE,
		"group- and canopy-wide filings are canopy's own",
	);
	CheckPolicy::register(
		conn,
		source,
		filing.check,
		filing.default_ceiling,
		filing.default_escalates,
		filing.documentation,
	)
	.await?;

	// Rule-evaluation context: the check's detail (with the normalised
	// result injected, mirroring status ingestion), no report-wide
	// extras, and the server's tags where there is a server. Filings
	// whose observation policy shouldn't touch (an operator explicitly
	// raising a manual condition) still flow through so the catalog row
	// and stamps exist, but manual entries register at a failed ceiling
	// so the operator's chosen result passes through ungraded by default.
	let mut check_extra = filing
		.detail
		.as_ref()
		.and_then(|d| d.as_object().cloned())
		.unwrap_or_default();
	check_extra.insert(
		"result".into(),
		serde_json::Value::String(filing.observed.to_string()),
	);
	let status_extra = serde_json::Map::new();
	let (tags, scope_server, scope_group): (
		std::collections::HashMap<String, serde_json::Value>,
		Option<Uuid>,
		Option<Uuid>,
	) = match filing.scope {
		FilingScope::Server { server_id, .. } => {
			let server = Server::get_by_id(conn, server_id).await?;
			let group_id = server.group_id;
			let tags = server
				.tags_merged_with_group(conn)
				.await?
				.0
				.into_iter()
				.map(|(k, v)| (k, serde_json::Value::String(v)))
				.collect();
			(tags, Some(server_id), group_id)
		}
		FilingScope::Group(group_id) => (Default::default(), None, Some(group_id)),
		FilingScope::Global => (Default::default(), None, None),
	};
	let ctx = EvaluationContext {
		status_extra: &status_extra,
		check_extra: &check_extra,
		tags: &tags,
	};
	let graded = CheckPolicy::apply_scoped(
		conn,
		source,
		filing.check,
		filing.observed,
		&ctx,
		scope_server,
		scope_group,
	)
	.await?;

	let active = matches!(
		graded.effective,
		CheckResult::Failed | CheckResult::Warning | CheckResult::Broken
	);
	let description = if active { filing.title } else { None };
	let stamp = CheckStateStamp {
		check: filing.check.to_string(),
		observed: filing.observed,
		effective: graded.effective,
		escalates: graded.escalates,
		detail: filing.detail.clone(),
	};

	match filing.scope {
		FilingScope::Server {
			server_id,
			device_id,
		} => {
			NewEvent {
				source: source.to_string(),
				r#ref: filing.check.to_string(),
				description: description.map(str::to_string),
				message: filing.message.to_string(),
				active: Some(active),
				occurred_at: None,
			}
			.save_with_state(conn, server_id, device_id, Some(&stamp), false)
			.await
		}
		FilingScope::Group(gid) => {
			raise_group_event_with_state(
				conn,
				gid,
				filing.check,
				description,
				filing.message,
				active,
				Some(&stamp),
			)
			.await
		}
		FilingScope::Global => {
			raise_global_event_with_state(
				conn,
				filing.check,
				description,
				filing.message,
				active,
				Some(&stamp),
			)
			.await
		}
	}
}

/// Per-server health rollup from current check state: the worst
/// effective result across every source's checks on the server — any
/// failure ⇒ unhealthy, otherwise any warning or brokenness ⇒ warning,
/// otherwise healthy. Silenced checks are skipped. Servers with no
/// check state are absent from the map (callers default them healthy,
/// matching the "no signal" semantics).
pub async fn health_from_check_state(
	conn: &mut AsyncPgConnection,
	servers: &[(Uuid, Option<Uuid>)],
) -> Result<std::collections::HashMap<Uuid, commons_types::status::HealthState>> {
	use crate::schema::{issues, scoped_check_policies};
	use commons_types::status::HealthState;
	use std::collections::{HashMap, HashSet};

	let mut out: HashMap<Uuid, HealthState> = HashMap::new();
	if servers.is_empty() {
		return Ok(out);
	}
	let server_ids: Vec<Uuid> = servers.iter().map(|(id, _)| *id).collect();
	let group_of: HashMap<Uuid, Option<Uuid>> = servers.iter().copied().collect();

	let rows: Vec<(Option<Uuid>, String, Option<String>, Option<String>)> = issues::table
		.select((
			issues::server_id,
			issues::source,
			issues::check_name,
			issues::effective_result,
		))
		.filter(issues::server_id.eq_any(&server_ids))
		.filter(issues::check_name.is_not_null())
		.filter(issues::effective_result.is_not_null())
		.load(conn)
		.await?;

	let group_ids: Vec<Uuid> = group_of.values().filter_map(|g| *g).collect();
	let silence_rows: Vec<(Option<Uuid>, Option<Uuid>, String, String)> =
		scoped_check_policies::table
			.select((
				scoped_check_policies::server_id,
				scoped_check_policies::server_group_id,
				scoped_check_policies::source,
				scoped_check_policies::check_name,
			))
			.filter(scoped_check_policies::ceiling.eq("skipped"))
			.filter(
				scoped_check_policies::server_id
					.eq_any(&server_ids)
					.or(scoped_check_policies::server_group_id.eq_any(&group_ids)),
			)
			.load(conn)
			.await?;
	let mut server_silences: HashSet<(Uuid, String, String)> = HashSet::new();
	let mut group_silences: HashSet<(Uuid, String, String)> = HashSet::new();
	for (server_id, group_id, source, check) in silence_rows {
		if let Some(sid) = server_id {
			server_silences.insert((sid, source, check));
		} else if let Some(gid) = group_id {
			group_silences.insert((gid, source, check));
		}
	}

	for (server_id, source, check_name, effective) in rows {
		let Some(server_id) = server_id else {
			continue;
		};
		let Some(check_name) = check_name else {
			continue;
		};
		let key = (server_id, source, check_name);
		if server_silences.contains(&key) {
			continue;
		}
		if let Some(Some(gid)) = group_of.get(&server_id)
			&& group_silences.contains(&(*gid, key.1.clone(), key.2.clone()))
		{
			continue;
		}
		let contribution = match effective.as_deref().and_then(|e| e.parse().ok()) {
			Some(CheckResult::Failed) => HealthState::Unhealthy,
			Some(CheckResult::Warning | CheckResult::Broken) => HealthState::Warning,
			_ => continue,
		};
		let entry = out.entry(server_id).or_insert(HealthState::Healthy);
		if contribution == HealthState::Unhealthy || *entry == HealthState::Healthy {
			*entry = contribution;
		}
	}

	Ok(out)
}

impl Issue {
	/// Check state for one (source, check), across every scope — server,
	/// group, and canopy-wide. The check detail page's data source: rows
	/// carry the observed/effective results, the check's detail, and the
	/// degraded-streak timestamps. A check's identity is the pair — a
	/// same-named check from another source is a different check.
	pub async fn check_state_for_check(
		conn: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
	) -> Result<Vec<Issue>> {
		use crate::schema::issues::dsl;

		dsl::issues
			.select(Issue::as_select())
			.filter(dsl::source.eq(source))
			.filter(dsl::check_name.eq(check_name))
			.filter(dsl::observed_result.is_not_null())
			.load(conn)
			.await
			.map_err(AppError::from)
	}
}

/// The canopy-wide issue at `(canopy, ref)`, if it has ever been raised.
pub async fn get_global_issue(conn: &mut AsyncPgConnection, r#ref: &str) -> Result<Option<Issue>> {
	use crate::schema::issues::dsl;

	dsl::issues
		.select(Issue::as_select())
		.filter(
			dsl::server_id
				.is_null()
				.and(dsl::server_group_id.is_null())
				.and(dsl::source.eq(crate::statuses::CANOPY_SOURCE))
				.and(dsl::ref_.eq(r#ref)),
		)
		.first(conn)
		.await
		.optional()
		.map_err(AppError::from)
}

/// Compute whether the issue *should* currently be contributing to an
/// open incident, and apply join/leave accordingly. The rules:
///
/// - **Leave**: `!active || resolved || snoozed || silenced || !monitored`.
///   A result downgrade alone does *not* remove an issue — once
///   contributing, it stays until it's actually gone or explicitly
///   suppressed. Flipping the server to unmonitored *does* remove it: the
///   operator has said they're not watching this server, so its issues
///   stop counting. The same applies if the check is silenced at server
///   or group scope — see [`crate::silenced_refs`].
/// - **Join**: not leaving, AND one of:
///   - the state opens incidents on its own — an effective failure (see
///     [`Issue::opens_incident`]); or
///   - the target already has an open incident — then any active issue,
///     warnings included, joins it. The threshold only governs incident
///     *creation*; once an incident is in progress everything else piles
///     in for context.
/// - **Close**: when the last effective-failure contributor leaves, the
///   incident starts **lingering** (`closing_at` stamped) rather than
///   closing: a failure returning within the target's linger window clears
///   the stamp and the same incident continues, so a red check that blips
///   green doesn't turn one span of trouble into a resolve/re-open pair.
///   The linger sweep ([`sweep_lingering_incidents`]) closes it once the
///   window elapses, backdating the close to when the failure left. Only a
///   leave caused by the check actually recovering lingers: resolution,
///   snooze, silence, and monitoring-off are explicit operator actions,
///   not flaps, and close immediately — as does a zero linger window.
///   Lesser contributors that joined because
///   the target had an open incident stay attached (so the audit trail and
///   Slack thread retain them) but **do not** hold the incident open (or
///   lingering) by themselves. Without this asymmetry, a check that's
///   stuck firing at warning could keep an incident open indefinitely
///   after the failure that opened it has long since gone away.
///
/// `monitored` reflects the server's `is_monitored()` at call time. When
/// `false`, the issue is treated as a "leave": this is what makes
/// `unmonitored` an opt-out of the incident workflow without losing the
/// issue rows themselves.
///
/// `by` is the operator login when this re-evaluation was triggered by a
/// human action (e.g. resolving an issue or incident). It's threaded
/// through so a cascade close caused by that action can attribute the
/// resulting Slack `incident_resolve` row to the operator rather than
/// "the healthcheck recovering". `None` for device-driven flows (event
/// push with `active:false`).
async fn re_evaluate_incident_membership(
	conn: &mut AsyncPgConnection,
	issue: &Issue,
	target: IncidentTarget,
	monitored: bool,
	transition_time: Timestamp,
	by: Option<&str>,
) -> Result<()> {
	use crate::schema::{incident_issues, incidents};

	let was_in = is_issue_in_open_incident(conn, issue.id).await?;
	let snoozed = issue.snoozed_until.map_or(false, |t| t > Timestamp::now());
	// A group-scoped issue (server_id = None) can only be silenced at the
	// group level; pass the nil server so only the group list is consulted.
	// Canopy-wide issues have no silence scope (yet).
	let silenced = crate::silenced_refs::is_silenced(
		conn,
		issue.server_id.unwrap_or(Uuid::nil()),
		target.group_id(),
		&issue.source,
		&issue.r#ref,
	)
	.await?;
	let target_open = target_has_open_incident(conn, target).await?;

	let should_leave =
		!issue.active || issue.resolved_at.is_some() || snoozed || silenced || !monitored;
	let should_join = monitored
		&& !silenced
		&& issue.active
		&& issue.resolved_at.is_none()
		&& !snoozed
		&& (issue.opens_incident() || target_open);

	match (was_in, should_join, should_leave) {
		(false, true, _) => {
			let (incident_id, newly_opened) =
				find_or_open_incident(conn, target, transition_time).await?;
			diesel::insert_into(incident_issues::table)
				.values((
					incident_issues::incident_id.eq(incident_id),
					incident_issues::issue_id.eq(issue.id),
					incident_issues::joined_at.eq(jiff_diesel::Timestamp::from(transition_time)),
				))
				.execute(conn)
				.await?;
			// An effective failure joining a lingering incident ends the
			// lingering: the trouble is back, the same incident continues.
			if !newly_opened && issue.opens_incident() {
				diesel::update(
					incidents::table
						.filter(incidents::id.eq(incident_id))
						.filter(incidents::closing_at.is_not_null()),
				)
				.set(incidents::closing_at.eq(None::<jiff_diesel::Timestamp>))
				.execute(conn)
				.await?;
			}
			if newly_opened {
				enqueue_slack_open(conn, incident_id, target, issue).await?;
			} else if issue.escalates_now() {
				// Two sub-cases when an escalating failure joins an existing incident:
				//  - The original open is still pending in the outbox →
				//    accelerate so the "incident opened" message lands
				//    immediately. No second message: the open hasn't been
				//    seen yet, so a fresh open would be redundant noise.
				//  - The original open has already shipped → enqueue a
				//    fresh open at Critical severity as the escalation
				//    signal. Gated on incidents.escalated_at IS NULL so
				//    repeated Critical joins (or a Critical leaving and
				//    rejoining) don't re-fire the message.
				let accelerated = accelerate_pending_open(conn, incident_id).await?;
				if !accelerated {
					let escalated: Option<Incident> = diesel::update(
						incidents::table
							.filter(incidents::id.eq(incident_id))
							.filter(incidents::escalated_at.is_null()),
					)
					.set(incidents::escalated_at.eq(jiff_diesel::Timestamp::from(transition_time)))
					.returning(Incident::as_select())
					.get_result(conn)
					.await
					.optional()?;
					if escalated.is_some() {
						enqueue_slack_open(conn, incident_id, target, issue).await?;
					}
				}
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

			// Serialize against concurrent leaves of *other* issues on
			// the same incident. Without this incident-row lock, two
			// transactions each removing one of the last two live
			// issues can each observe remaining_open >= 1 (each sees
			// its own in-flight left_at update but not the other's)
			// and skip the close, leaving the incident in "no live
			// issues but closed_at IS NULL" with no Slack fired.
			let _incident_lock: Uuid = incidents::table
				.select(incidents::id)
				.filter(incidents::id.eq(open_link.incident_id))
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

			// Only count contributors that *currently* open an incident
			// (effective failures). Lesser contributors stay attached for
			// context but don't hold the incident open on their own; see
			// the function doc-comment for the rationale.
			use crate::schema::issues;
			let remaining_open: i64 = incident_issues::table
				.inner_join(issues::table.on(issues::id.eq(incident_issues::issue_id)))
				.filter(
					incident_issues::incident_id
						.eq(open_link.incident_id)
						.and(incident_issues::left_at.is_null())
						.and(issues::effective_result.eq("failed")),
				)
				.count()
				.get_result(conn)
				.await?;
			if remaining_open == 0 {
				// Linger damps *reporter* flapping: only a leave caused by
				// the check actually recovering (inactive, with no operator
				// suppression in play) waits out the window. Resolution,
				// snooze, silence, and monitoring-off are explicit operator
				// actions — not flaps — and close immediately, Slack resolve
				// attributed where `by` is known. A zero window is the
				// operator opting out of lingering.
				let check_recovery = !issue.active
					&& issue.resolved_at.is_none()
					&& !snoozed && !silenced
					&& monitored;
				let window = linger_window(conn, target).await?;
				if by.is_some() || !check_recovery || window.is_zero() {
					//
					// Filter on `closed_at IS NULL` so that when a stranded
					// lesser contributor eventually leaves an already-closed
					// incident (because the failure-filter close above already
					// retired it), we skip both the no-op update and the
					// double Slack resolve.
					let closed: Option<Incident> = diesel::update(
						incidents::table
							.filter(incidents::id.eq(open_link.incident_id))
							.filter(incidents::closed_at.is_null()),
					)
					.set(incidents::closed_at.eq(jiff_diesel::Timestamp::from(transition_time)))
					.returning(Incident::as_select())
					.get_result(conn)
					.await
					.optional()?;
					if let Some(closed) = closed {
						enqueue_slack_resolve_inner(conn, &closed, by).await?;
					}
				} else {
					// Start lingering: record when the last effective failure
					// left. Stamped once — a lesser contributor leaving an
					// already-lingering incident doesn't move the mark — and
					// the linger sweep closes the incident when the stamp
					// outlives the window (see `sweep_lingering_incidents`).
					diesel::update(
						incidents::table
							.filter(incidents::id.eq(open_link.incident_id))
							.filter(incidents::closed_at.is_null())
							.filter(incidents::closing_at.is_null()),
					)
					.set(incidents::closing_at.eq(jiff_diesel::Timestamp::from(transition_time)))
					.execute(conn)
					.await?;
				}
			}
		}
		_ => {
			// A member issue re-filing as an effective failure while its
			// incident lingers ends the lingering — the trouble is back. A
			// leave-and-rejoin lands in the join arm above; this catches the
			// member that never left, e.g. a warning contributor re-graded
			// to failed.
			if was_in && should_join && !should_leave && issue.opens_incident() {
				let member_of: Vec<Uuid> = incident_issues::table
					.select(incident_issues::incident_id)
					.filter(incident_issues::issue_id.eq(issue.id))
					.filter(incident_issues::left_at.is_null())
					.load(conn)
					.await?;
				diesel::update(
					incidents::table
						.filter(incidents::id.eq_any(member_of))
						.filter(incidents::closed_at.is_null())
						.filter(incidents::closing_at.is_not_null()),
				)
				.set(incidents::closing_at.eq(None::<jiff_diesel::Timestamp>))
				.execute(conn)
				.await?;
			}
		}
	}
	Ok(())
}

/// Resolve the `(target, monitored)` pair an issue should be re-evaluated
/// against, handling all three scopes.
///
/// - Server-scoped (`server_id = Some`): look the server up; its `group_id`
///   and `is_monitored` drive the evaluation. `None` group → ungrouped, no
///   incident path.
/// - Group-scoped (`server_group_id = Some`): use the group directly and force
///   `monitored = true` so the per-server gate never silences a control-plane
///   issue.
/// - Canopy-wide (neither): the global target, always monitored.
async fn issue_target_and_monitored(
	conn: &mut AsyncPgConnection,
	issue: &Issue,
) -> Result<Option<(IncidentTarget, bool)>> {
	match (issue.server_id, issue.server_group_id) {
		(_, Some(gid)) => Ok(Some((IncidentTarget::Group(gid), true))),
		(Some(sid), None) => {
			let server = Server::get_by_id(conn, sid).await?;
			Ok(server
				.group_id
				.map(|gid| (IncidentTarget::Group(gid), server.is_monitored)))
		}
		(None, None) => Ok(Some((IncidentTarget::Global, true))),
	}
}

/// Re-evaluate every currently-open issue on `server_id` against incident
/// membership. Used as a catch-up when the operator flips a property of
/// the server that changes its incident eligibility — currently:
/// ungrouped→grouped (via [`Server::assign_to_group`]) and monitored
/// toggles (via the private-server's server update handler). On flips
/// that should *remove* issues from an incident (e.g. monitored→
/// unmonitored), `re_evaluate_incident_membership` will leave them and
/// close the incident if nothing else props it up.
pub async fn reevaluate_open_issues_for_server(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
) -> Result<()> {
	use crate::schema::issues::dsl;

	let server = Server::get_by_id(db, server_id).await?;
	let Some(gid) = server.group_id else {
		return Ok(());
	};
	let monitored = server.is_monitored;

	let open_issues: Vec<Issue> = dsl::issues
		.select(Issue::as_select())
		.filter(dsl::server_id.eq(server_id))
		.filter(dsl::active.eq(true))
		.filter(dsl::resolved_at.is_null())
		.load(db)
		.await?;

	let now = Timestamp::now();
	for issue in open_issues {
		re_evaluate_incident_membership(
			db,
			&issue,
			IncidentTarget::Group(gid),
			monitored,
			now,
			None,
		)
		.await?;
	}
	Ok(())
}

/// Enqueue `server_id` for deferred incident (re-)evaluation.
///
/// The device status-ingest path calls this in place of the inline
/// `re_evaluate_incident_membership` it used to run: recording the issue
/// state is kept synchronous, but the incident work — which takes the
/// per-group `server_groups` lock — is handed to the queue worker so
/// request traffic never contends on that lock.
///
/// Idempotent: the primary key coalesces a burst of pushes into one queued
/// unit. A push arriving while a re-evaluation is already pending is a
/// no-op — the pending run reads current state when it fires, so it already
/// covers the later push.
pub async fn enqueue_incident_reeval(conn: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
	use crate::schema::incident_reeval_queue::dsl;
	diesel::insert_into(dsl::incident_reeval_queue)
		.values(dsl::server_id.eq(server_id))
		.on_conflict(dsl::server_id)
		.do_nothing()
		.execute(conn)
		.await?;
	Ok(())
}

/// Re-evaluate incident membership for every issue on `server_id` that can
/// currently affect an incident: issues that are active-and-unresolved
/// (candidates to *join*), plus issues still linked to an open incident
/// (candidates to *leave* — e.g. a check that just recovered and went
/// inactive). [`reevaluate_open_issues_for_server`] deliberately only walks
/// active issues, so it can't drive the leave transitions the ingest path
/// needs; this is the ingest-equivalent set.
///
/// Active issues are evaluated before inactive ones so that when a failure
/// replaces another within a single push, the incoming failure (re)joins
/// the incident before the outgoing one leaves — otherwise the incident
/// would briefly close and reopen. This mirrors the ingest-time filing
/// order.
pub async fn reevaluate_incidents_for_server(
	conn: &mut AsyncPgConnection,
	server_id: Uuid,
) -> Result<()> {
	use crate::schema::{incident_issues, incidents, issues};

	let server = Server::get_by_id(conn, server_id).await?;
	let Some(gid) = server.group_id else {
		return Ok(());
	};
	let monitored = server.is_monitored;

	let mut candidates: Vec<Issue> = issues::table
		.select(Issue::as_select())
		.filter(issues::server_id.eq(server_id))
		.filter(
			issues::active
				.eq(true)
				.and(issues::resolved_at.is_null())
				.or(issues::id.eq_any(
					incident_issues::table
						.inner_join(
							incidents::table.on(incidents::id.eq(incident_issues::incident_id)),
						)
						.filter(incident_issues::left_at.is_null())
						.filter(incidents::closed_at.is_null())
						.select(incident_issues::issue_id),
				)),
		)
		.load(conn)
		.await?;

	// Active (potential joiners) before inactive (leavers).
	candidates.sort_by_key(|issue| !issue.active);

	let now = Timestamp::now();
	for issue in candidates {
		re_evaluate_incident_membership(
			conn,
			&issue,
			IncidentTarget::Group(gid),
			monitored,
			now,
			None,
		)
		.await?;
	}
	Ok(())
}

/// Drain the incident re-evaluation queue, running
/// [`reevaluate_incidents_for_server`] for up to `limit` queued servers and
/// removing each once handled. Returns the number of servers processed.
///
/// Each server is claimed and processed in its own transaction with
/// `FOR UPDATE SKIP LOCKED`, so a slow re-evaluation doesn't hold a batch of
/// queue rows locked, overlapping ticks don't double-process, and a failure
/// on one server doesn't roll back the others.
pub async fn process_incident_reeval_queue(
	conn: &mut AsyncPgConnection,
	limit: i64,
) -> Result<usize> {
	use crate::schema::incident_reeval_queue::dsl;

	let mut processed = 0usize;
	while (processed as i64) < limit {
		let claimed: Option<Uuid> = conn
			.transaction::<_, AppError, _>(async |tx| {
				let Some(server_id): Option<Uuid> = dsl::incident_reeval_queue
					.select(dsl::server_id)
					.order(dsl::enqueued_at.asc())
					.for_update()
					.skip_locked()
					.first(tx)
					.await
					.optional()?
				else {
					return Ok(None);
				};
				reevaluate_incidents_for_server(tx, server_id).await?;
				diesel::delete(dsl::incident_reeval_queue.filter(dsl::server_id.eq(server_id)))
					.execute(tx)
					.await?;
				Ok(Some(server_id))
			})
			.await?;
		match claimed {
			Some(_) => processed += 1,
			None => break,
		}
	}
	Ok(processed)
}

/// Re-evaluate every currently-open issue on `server_id` with the given
/// `(source, ref)`. Narrower variant of [`reevaluate_open_issues_for_server`]
/// used after a server-scoped silence is added or removed: only the matching
/// refs need to revisit their incident membership.
pub async fn reevaluate_open_issues_for_server_ref(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	source: &str,
	r#ref: &str,
) -> Result<()> {
	use crate::schema::issues::dsl;

	let server = Server::get_by_id(db, server_id).await?;
	let Some(gid) = server.group_id else {
		return Ok(());
	};
	let monitored = server.is_monitored;

	let open_issues: Vec<Issue> = dsl::issues
		.select(Issue::as_select())
		.filter(dsl::server_id.eq(server_id))
		.filter(dsl::source.eq(source))
		.filter(dsl::ref_.eq(r#ref))
		.filter(dsl::active.eq(true))
		.filter(dsl::resolved_at.is_null())
		.load(db)
		.await?;

	let now = Timestamp::now();
	for issue in open_issues {
		re_evaluate_incident_membership(
			db,
			&issue,
			IncidentTarget::Group(gid),
			monitored,
			now,
			None,
		)
		.await?;
	}
	Ok(())
}

/// Re-evaluate every currently-open issue in the group with the given
/// `(source, ref)`. Used after a group-scoped silence is added or removed.
pub async fn reevaluate_open_issues_for_group_ref(
	db: &mut AsyncPgConnection,
	server_group_id: Uuid,
	source: &str,
	r#ref: &str,
) -> Result<()> {
	use crate::schema::{issues, servers};

	let server_ids: Vec<Uuid> = servers::table
		.select(servers::id)
		.filter(servers::group_id.eq(server_group_id))
		.load(db)
		.await?;

	// Both the group's server-scoped issues and any group-scoped issues
	// (server_group_id = this group) that match the silenced (source, ref).
	let open_issues: Vec<Issue> = issues::table
		.select(Issue::as_select())
		.filter(
			issues::server_id
				.eq_any(&server_ids)
				.or(issues::server_group_id.eq(server_group_id)),
		)
		.filter(issues::source.eq(source))
		.filter(issues::ref_.eq(r#ref))
		.filter(issues::active.eq(true))
		.filter(issues::resolved_at.is_null())
		.load(db)
		.await?;

	let now = Timestamp::now();
	// Servers in the same group all share the same `monitored` only by
	// coincidence; look each up so the re-evaluation sees current state.
	let mut monitored_by_server: std::collections::HashMap<Uuid, bool> =
		std::collections::HashMap::new();
	for sid in &server_ids {
		let s = Server::get_by_id(db, *sid).await?;
		monitored_by_server.insert(*sid, s.is_monitored);
	}
	for issue in open_issues {
		// Group-scoped issues bypass the per-server monitored gate.
		let monitored = match issue.server_id {
			Some(sid) => monitored_by_server.get(&sid).copied().unwrap_or(true),
			None => true,
		};
		re_evaluate_incident_membership(
			db,
			&issue,
			IncidentTarget::Group(server_group_id),
			monitored,
			now,
			None,
		)
		.await?;
	}
	Ok(())
}

/// Re-evaluate every currently-open issue across every server in the
/// database against incident membership. Intended as a startup pass for
/// background workers: if the rules around incident eligibility change
/// in code (or a migration silently bumped state the running code now
/// reads differently — see PR #170), this drives the actual incident
/// rows into agreement with what the current code says they should be.
///
/// Idempotent. Cheap when the database is already consistent — each
/// issue's `re_evaluate_incident_membership` call short-circuits in the
/// `_ => {}` arm when the incident state already matches.
///
/// Runs as a single transaction so a crash mid-reconcile doesn't leave
/// the incident state half-fixed. The per-issue FOR UPDATE locks held
/// inside `re_evaluate_incident_membership` accumulate across the
/// transaction's lifetime — fine at canopy's scale (hundreds of open
/// issues at most) but worth keeping in mind if the corpus grows.
///
/// Returns `(servers_walked, issues_evaluated)` for the caller to log.
pub async fn reconcile_open_incidents(db: &mut AsyncPgConnection) -> Result<(usize, usize)> {
	use crate::schema::issues::dsl;

	db.transaction::<_, AppError, _>(async |conn| {
		let open_issues: Vec<Issue> = dsl::issues
			.select(Issue::as_select())
			.filter(dsl::active.eq(true))
			.filter(dsl::resolved_at.is_null())
			.load(conn)
			.await?;

		if open_issues.is_empty() {
			return Ok((0, 0));
		}

		let server_ids: Vec<Uuid> = {
			let mut s: std::collections::HashSet<Uuid> =
				open_issues.iter().filter_map(|i| i.server_id).collect();
			s.drain().collect()
		};
		let servers = Server::get_by_ids(conn, &server_ids).await?;
		let by_id: std::collections::HashMap<Uuid, Server> =
			servers.into_iter().map(|s| (s.id, s)).collect();

		let now = Timestamp::now();
		let mut evaluated = 0usize;
		for issue in open_issues {
			match (issue.server_id, issue.server_group_id) {
				// Group-scoped issue: resolve its group directly, bypass the
				// per-server is_monitored gate (monitored = true).
				(None, Some(gid)) => {
					re_evaluate_incident_membership(
						conn,
						&issue,
						IncidentTarget::Group(gid),
						true,
						now,
						None,
					)
					.await?;
					evaluated += 1;
				}
				// Server-scoped issue: look up the server and its group.
				(Some(sid), _) => {
					let Some(server) = by_id.get(&sid) else {
						continue;
					};
					let Some(gid) = server.group_id else {
						// Ungrouped servers can't have incidents; nothing to reconcile.
						continue;
					};
					re_evaluate_incident_membership(
						conn,
						&issue,
						IncidentTarget::Group(gid),
						server.is_monitored,
						now,
						None,
					)
					.await?;
					evaluated += 1;
				}
				// Canopy-wide issue: the global target, always monitored.
				(None, None) => {
					re_evaluate_incident_membership(
						conn,
						&issue,
						IncidentTarget::Global,
						true,
						now,
						None,
					)
					.await?;
					evaluated += 1;
				}
			}
		}
		Ok((by_id.len(), evaluated))
	})
	.await
}
/// Close incidents whose linger window has expired: `closing_at` (when the
/// last effective failure left) has outlived the target's window without a
/// failure returning. The close is backdated to `closing_at` — the linger
/// is damping machinery, not part of the incident's span — and the Slack
/// cancel-or-resolve runs exactly as an immediate close would have:
/// a never-shipped open is cancelled (the flap stays silent), a shipped
/// open gets its resolve.
///
/// Runs on the monitor pod's minute cadence. Each close is its own
/// transaction, re-checked under a row lock: a failure rejoining (which
/// clears `closing_at`) or an operator resolve (which sets `closed_at`)
/// between the scan and the lock wins. Belt-and-braces, a lingering
/// incident that somehow still has a live effective-failure member is
/// un-lingered instead of closed.
///
/// Returns the number of incidents closed.
pub async fn sweep_lingering_incidents(db: &mut AsyncPgConnection) -> Result<usize> {
	use crate::schema::{incident_issues, incidents, issues};

	let now = Timestamp::now();
	let candidates: Vec<Incident> = incidents::table
		.select(Incident::as_select())
		.filter(incidents::closed_at.is_null())
		.filter(incidents::closing_at.is_not_null())
		.load(db)
		.await?;

	let mut closed = 0usize;
	for candidate in candidates {
		let did_close = db
			.transaction::<_, AppError, _>(async |conn| {
				let incident: Option<Incident> = incidents::table
					.select(Incident::as_select())
					.filter(incidents::id.eq(candidate.id))
					.filter(incidents::closed_at.is_null())
					.for_update()
					.first(conn)
					.await
					.optional()?;
				let Some(incident) = incident else {
					return Ok(false);
				};
				let Some(closing_at) = incident.closing_at else {
					return Ok(false);
				};
				let target = IncidentTarget::of_incident(&incident);
				if closing_at + linger_window(conn, target).await? > now {
					return Ok(false);
				}
				let live_failures: i64 = incident_issues::table
					.inner_join(issues::table.on(issues::id.eq(incident_issues::issue_id)))
					.filter(incident_issues::incident_id.eq(incident.id))
					.filter(incident_issues::left_at.is_null())
					.filter(issues::effective_result.eq("failed"))
					.count()
					.get_result(conn)
					.await?;
				if live_failures > 0 {
					diesel::update(incidents::table.filter(incidents::id.eq(incident.id)))
						.set(incidents::closing_at.eq(None::<jiff_diesel::Timestamp>))
						.execute(conn)
						.await?;
					return Ok(false);
				}
				let closed_incident: Incident =
					diesel::update(incidents::table.filter(incidents::id.eq(incident.id)))
						.set(incidents::closed_at.eq(jiff_diesel::Timestamp::from(closing_at)))
						.returning(Incident::as_select())
						.get_result(conn)
						.await?;
				enqueue_slack_resolve_inner(conn, &closed_incident, None).await?;
				Ok(true)
			})
			.await?;
		if did_close {
			closed += 1;
		}
	}
	Ok(closed)
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

/// What an issue's incident contribution attaches to: its server's group,
/// or canopy as a whole for canopy-wide issues (self-alerts). Issues on
/// ungrouped servers have no target and no incident path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncidentTarget {
	Group(Uuid),
	Global,
}

impl IncidentTarget {
	/// The `incidents.server_group_id` value for this target.
	fn group_id(self) -> Option<Uuid> {
		match self {
			Self::Group(gid) => Some(gid),
			Self::Global => None,
		}
	}

	/// The target an existing incident row belongs to.
	pub fn of_incident(incident: &Incident) -> Self {
		match incident.server_group_id {
			Some(gid) => Self::Group(gid),
			None => Self::Global,
		}
	}
}

async fn target_has_open_incident(
	db: &mut AsyncPgConnection,
	target: IncidentTarget,
) -> Result<bool> {
	use crate::schema::incidents::dsl;

	let mut q = dsl::incidents.filter(dsl::closed_at.is_null()).into_boxed();
	q = match target {
		IncidentTarget::Group(gid) => q.filter(dsl::server_group_id.eq(gid)),
		IncidentTarget::Global => q.filter(dsl::server_group_id.is_null()),
	};
	let count: i64 = q.count().get_result(db).await?;
	Ok(count > 0)
}

/// Returns the open incident's id and whether this call newly opened it.
/// The boolean is consumed by `re_evaluate_incident_membership` to decide
/// whether a Slack `incident_open` outbox row should be enqueued (re-joining
/// an existing incident shouldn't re-notify).
///
/// Two parallel event pushes against the same target must not each insert
/// a fresh incident row. For a group target we take a `FOR UPDATE` lock on
/// the group's row up-front so the find-or-create pair serializes
/// per-group; the global target has no row to lock, so it serializes on a
/// transaction-scoped advisory lock instead. The unique partial indexes on
/// open incidents are the belt-and-braces backstop at the DB layer.
async fn find_or_open_incident(
	db: &mut AsyncPgConnection,
	target: IncidentTarget,
	opened_at: Timestamp,
) -> Result<(Uuid, bool)> {
	use crate::schema::{incidents, server_groups};

	match target {
		IncidentTarget::Group(gid) => {
			let _group_lock: Uuid = server_groups::table
				.select(server_groups::id)
				.filter(server_groups::id.eq(gid))
				.for_update()
				.first(db)
				.await?;
		}
		IncidentTarget::Global => {
			// Arbitrary constant, stable across releases: the one global
			// incident slot.
			diesel::sql_query("SELECT pg_advisory_xact_lock(818_723_001)")
				.execute(db)
				.await?;
		}
	}

	let mut q = incidents::table
		.select(Incident::as_select())
		.filter(incidents::closed_at.is_null())
		.into_boxed();
	q = match target {
		IncidentTarget::Group(gid) => q.filter(incidents::server_group_id.eq(gid)),
		IncidentTarget::Global => q.filter(incidents::server_group_id.is_null()),
	};
	let open: Option<Incident> = q
		.order(incidents::opened_at.desc())
		.first(db)
		.await
		.optional()?;
	if let Some(inc) = open {
		return Ok((inc.id, false));
	}

	let new_incident: Incident = diesel::insert_into(incidents::table)
		.values((
			incidents::server_group_id.eq(target.group_id()),
			incidents::opened_at.eq(jiff_diesel::Timestamp::from(opened_at)),
		))
		.returning(Incident::as_select())
		.get_result(db)
		.await?;
	Ok((new_incident.id, true))
}

/// Flap grace for canopy-wide incidents, mirroring the per-group
/// `slack_open_delay` default (the global target has no config row).
const GLOBAL_OPEN_GRACE: SignedDuration = SignedDuration::from_secs(3 * 60);

/// Linger window for canopy-wide incidents, mirroring the per-group
/// `slack_close_delay` default (the global target has no config row).
const GLOBAL_CLOSE_GRACE: SignedDuration = SignedDuration::from_secs(5 * 60);

/// The target's linger window: how long an incident outlives its last
/// effective failure before it closes.
async fn linger_window(
	conn: &mut AsyncPgConnection,
	target: IncidentTarget,
) -> Result<SignedDuration> {
	Ok(match target {
		IncidentTarget::Group(gid) => ServerGroup::get_by_id(conn, gid).await?.slack_close_delay.0,
		IncidentTarget::Global => GLOBAL_CLOSE_GRACE,
	})
}

async fn enqueue_slack_open(
	conn: &mut AsyncPgConnection,
	incident_id: Uuid,
	target: IncidentTarget,
	issue: &Issue,
) -> Result<()> {
	let (label, open_delay) = match target {
		IncidentTarget::Group(gid) => {
			let group = ServerGroup::get_by_id(conn, gid).await?;
			let server = match issue.server_id {
				Some(sid) => Some(Server::get_by_id(conn, sid).await?),
				None => None,
			};
			(
				format_group_label(&group, server.as_ref()),
				group.slack_open_delay.0,
			)
		}
		IncidentTarget::Global => ("Canopy".to_string(), GLOBAL_OPEN_GRACE),
	};
	// The deployed Slack workflow's trigger declares a `severity`
	// variable; feed it the result-derived urgency label.
	let urgency = if issue.escalates_now() {
		"Critical"
	} else if issue.opens_incident() {
		"Error"
	} else {
		"Warning"
	};
	let payload = crate::slack_outbox::vars::incident_open(
		&label,
		urgency,
		&issue.source,
		&issue.r#ref,
		&issue.message,
	);
	// Normally the row sits in the outbox for the target's open delay
	// before the drainer can ship it — that's the flap-suppression
	// window, so a transient open/close pair never reaches Slack.
	// An escalating failure bypasses this: the operator has marked the
	// check's policy as escalating — they don't want any delay.
	let deliver_after = if issue.escalates_now() {
		Timestamp::now()
	} else {
		Timestamp::now() + open_delay
	};
	crate::slack_outbox::SlackOutbox::enqueue(
		conn,
		crate::slack_outbox::KIND_INCIDENT_OPEN,
		Some(incident_id),
		Some(issue.id),
		None,
		payload,
		deliver_after,
	)
	.await?;
	Ok(())
}

/// Pull forward the `deliver_after` of any pending `incident_open` row
/// for `incident_id` to now. Used when a Critical-severity issue joins
/// an incident whose open was originally enqueued by a lower-severity
/// contributor — the operator's signal is "no more delay".
///
/// Affects only rows that are still in their delay window
/// (`delivered_at IS NULL`, `gave_up_at IS NULL`, `deliver_after > now`).
/// Already-shipped or cancelled rows are left alone.
///
/// Returns true if at least one row was accelerated. The caller uses
/// this to distinguish "open is still pending" (no extra Slack message
/// needed) from "open has already shipped" (fire an escalation open).
async fn accelerate_pending_open(conn: &mut AsyncPgConnection, incident_id: Uuid) -> Result<bool> {
	use crate::schema::slack_outbox::dsl;
	let now = Timestamp::now();
	let updated = diesel::update(
		dsl::slack_outbox
			.filter(dsl::incident_id.eq(Some(incident_id)))
			.filter(dsl::kind.eq(crate::slack_outbox::KIND_INCIDENT_OPEN))
			.filter(dsl::delivered_at.is_null())
			.filter(dsl::gave_up_at.is_null())
			.filter(dsl::deliver_after.gt(jiff_diesel::Timestamp::from(now))),
	)
	.set(dsl::deliver_after.eq(jiff_diesel::Timestamp::from(now)))
	.execute(conn)
	.await
	.map_err(AppError::from)?;
	Ok(updated > 0)
}

async fn enqueue_slack_resolve(
	conn: &mut AsyncPgConnection,
	incident: &Incident,
	by: &str,
) -> Result<()> {
	enqueue_slack_resolve_inner(conn, incident, Some(by)).await
}

async fn enqueue_slack_resolve_inner(
	conn: &mut AsyncPgConnection,
	incident: &Incident,
	by: Option<&str>,
) -> Result<()> {
	// If the matching open is still inside its `deliver_after` window
	// (or has otherwise not been delivered yet) we cancel it and skip
	// posting the resolve too: Slack never heard about the incident,
	// so there's nothing to "resolve" there. Once the drainer has shipped
	// the open this update affects zero rows and we fall through to the
	// normal enqueue.
	let cancelled = crate::slack_outbox::SlackOutbox::cancel_pending_open(
		conn,
		incident.id,
		"cancelled: incident resolved before the open had been delivered to Slack",
	)
	.await?;
	if cancelled > 0 {
		return Ok(());
	}
	let label = match incident.server_group_id {
		Some(gid) => {
			let group = ServerGroup::get_by_id(conn, gid).await?;
			format_group_label(&group, None)
		}
		None => "Canopy".to_string(),
	};
	let payload = crate::slack_outbox::vars::incident_resolve(&label, by);
	crate::slack_outbox::SlackOutbox::enqueue(
		conn,
		crate::slack_outbox::KIND_INCIDENT_RESOLVE,
		Some(incident.id),
		None,
		None,
		payload,
		Timestamp::now(),
	)
	.await?;
	Ok(())
}

fn format_group_label(group: &ServerGroup, server: Option<&Server>) -> String {
	if let Some(server) = server {
		let host = server.host.as_ref().map(|h| h.0.to_string());
		let server_part = match (&server.name, host) {
			(Some(n), Some(h)) if !n.is_empty() => format!("{n} ({h})"),
			(Some(n), None) if !n.is_empty() => n.clone(),
			(_, Some(h)) => h,
			(_, None) => server.id.to_string(),
		};
		format!("{} · {}", group.name, server_part)
	} else {
		group.name.clone()
	}
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
			// Healthy check state (rows that never degraded) isn't an
			// issue; it has its own read surfaces.
			.filter(dsl::active.eq(true).or(dsl::last_degraded_at.is_not_null()))
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
			// Healthy check state (rows that never degraded) isn't an
			// issue; it has its own read surfaces.
			.filter(dsl::active.eq(true).or(dsl::last_degraded_at.is_not_null()))
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
	/// - `results`: when `Some` and non-empty, restrict to issues whose
	///   latest effective result is one of those.
	/// - `server_group_id`: when `Some`, restrict to issues whose server is
	///   in that group. A direct `IN (SELECT id FROM servers WHERE group_id = $)`.
	pub async fn list(
		db: &mut AsyncPgConnection,
		filters: IssueListFilters,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::issues::dsl;
		use crate::schema::servers;

		let mut q = dsl::issues
			.select(Self::as_select())
			// Canopy-wide issues (self-alerts: neither server- nor
			// group-scoped) have their own surface (`crate::self_alerts`);
			// they are not fleet issues.
			.filter(
				dsl::server_id
					.is_not_null()
					.or(dsl::server_group_id.is_not_null()),
			)
			// Healthy check state (rows that never degraded) isn't an
			// issue; it has its own read surfaces.
			.filter(dsl::active.eq(true).or(dsl::last_degraded_at.is_not_null()))
			.into_boxed();
		if filters.active_only {
			q = q
				.filter(dsl::active.eq(true))
				.filter(dsl::resolved_at.is_null());
		}
		if let Some(results) = filters.results.as_ref().filter(|v| !v.is_empty()) {
			let strs: Vec<String> = results.iter().map(|r| r.to_string()).collect();
			q = q.filter(dsl::effective_result.eq_any(strs));
		}
		if let Some(gid) = filters.server_group_id {
			let server_ids: Vec<Uuid> = servers::table
				.select(servers::id)
				.filter(servers::group_id.eq(gid))
				.load(db)
				.await?;
			q = q.filter(dsl::server_id.eq_any(server_ids));
		}
		if let Some(since) = filters.since {
			q = q.filter(dsl::last_seen.ge(jiff_diesel::Timestamp::from(since)));
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

	/// Refs of the server's *active* issues under `source` whose ref
	/// starts with `prefix`. Used by status ingestion to decide which
	/// per-check issues to close: deriving closes from what's actually
	/// open (rather than diffing against the previous status row)
	/// stays correct across interludes that keep an issue open without
	/// re-filing it — e.g. failed → broken → passed, where the failure
	/// issue must close on the `passed` push even though the previous
	/// push didn't report a failure.
	pub async fn active_refs_with_prefix(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		source: &str,
		prefix: &str,
	) -> Result<Vec<String>> {
		use crate::schema::issues::dsl;
		debug_assert!(
			!prefix.contains(['%', '_', '\\']),
			"prefix is used in a LIKE pattern and must not contain wildcards"
		);
		dsl::issues
			.select(dsl::ref_)
			.filter(
				dsl::server_id
					.eq(server_id)
					.and(dsl::source.eq(source))
					.and(dsl::ref_.like(format!("{prefix}%")))
					.and(dsl::active.eq(true)),
			)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Bulk lookup of issues that share the same `(source, ref)` across many
	/// servers. Each `(server_id, source, ref)` is unique, so at most one row
	/// per server is returned. Used by the canopy reachability sweep.
	/// Group ids carrying an active group-scoped issue with this
	/// `(source, ref)`. Used by sweeps to recover only where an alert is
	/// live, keeping their idle path read-only.
	pub async fn active_group_ids_by_source_ref(
		db: &mut AsyncPgConnection,
		source: &str,
		ref_: &str,
	) -> Result<Vec<Uuid>> {
		use crate::schema::issues::dsl;
		dsl::issues
			.filter(dsl::source.eq(source))
			.filter(dsl::ref_.eq(ref_))
			.filter(dsl::active.eq(true))
			.filter(dsl::server_group_id.is_not_null())
			.select(dsl::server_group_id.assume_not_null())
			.load(db)
			.await
			.map_err(AppError::from)
	}

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

	/// Like [`Self::list_by_source_ref`], but matching any of several refs
	/// at once. Used by the staleness sweep, whose per-source check refs
	/// are only known at runtime.
	pub async fn list_by_source_refs(
		db: &mut AsyncPgConnection,
		source: &str,
		refs: &[String],
		server_ids: &[Uuid],
	) -> Result<Vec<Self>> {
		use crate::schema::issues::dsl;
		if server_ids.is_empty() || refs.is_empty() {
			return Ok(Vec::new());
		}
		dsl::issues
			.select(Self::as_select())
			.filter(
				dsl::source
					.eq(source)
					.and(dsl::ref_.eq_any(refs))
					.and(dsl::server_id.eq_any(server_ids)),
			)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Most recent check-state stamp per (server, reporting source): every
	/// report a source pushes re-stamps `last_seen` on the state rows of
	/// the checks it mentions, so the max per source is when that source
	/// last reported — maintained incrementally by ingestion, with no scan
	/// of the statuses history. The reserved sources are excluded (canopy
	/// and manual filings aren't reports), as are rows never stamped by
	/// the check-state model.
	pub async fn source_freshness(
		db: &mut AsyncPgConnection,
		server_ids: &[Uuid],
	) -> Result<Vec<(Uuid, String, Timestamp)>> {
		use crate::schema::issues::dsl;
		if server_ids.is_empty() {
			return Ok(Vec::new());
		}
		let rows: Vec<(Uuid, String, jiff_diesel::Timestamp)> = dsl::issues
			.filter(
				dsl::server_id
					.eq_any(server_ids)
					.and(
						dsl::source
							.ne_all([crate::statuses::CANOPY_SOURCE, crate::issues::MANUAL_SOURCE]),
					)
					.and(dsl::check_name.is_not_null()),
			)
			.group_by((dsl::server_id, dsl::source))
			.select((
				dsl::server_id.assume_not_null(),
				dsl::source,
				diesel::dsl::max(dsl::last_seen).assume_not_null(),
			))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows
			.into_iter()
			.map(|(server, source, seen)| (server, source, seen.into()))
			.collect())
	}

	/// Mark an issue as operator-resolved. Triggers incident-membership
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
			if let Some((target, monitored)) = issue_target_and_monitored(conn, &issue).await? {
				re_evaluate_incident_membership(conn, &issue, target, monitored, now, Some(by))
					.await?;
			}
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
			// Unresolve rejoins; cascade close path doesn't fire here.
			if let Some((target, monitored)) = issue_target_and_monitored(conn, &issue).await? {
				re_evaluate_incident_membership(conn, &issue, target, monitored, now, None).await?;
			}
			Ok(issue)
		})
		.await
	}

	/// Snooze an issue until the given timestamp. While snoozed, the issue
	/// can't open or join incidents. Triggers re-evaluation.
	///
	/// Snooze doesn't carry an operator login today, so a cascade close
	/// triggered by snoozing the last live issue still attributes the
	/// Slack resolve to "the healthcheck recovering" rather than the
	/// operator. Worth revisiting if/when snooze records its `by`.
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
			if let Some((target, monitored)) = issue_target_and_monitored(conn, &issue).await? {
				re_evaluate_incident_membership(conn, &issue, target, monitored, now, None).await?;
			}
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
			if let Some((target, monitored)) = issue_target_and_monitored(conn, &issue).await? {
				re_evaluate_incident_membership(conn, &issue, target, monitored, now, None).await?;
			}
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
	/// `incident_notes` for this incident + `issue_notes` across all linked issues.
	pub note_count: i64,
}

impl Incident {
	/// Bulk-fetch stats for a set of incidents. Missing incident_ids get
	/// `Default` (zero).
	///
	/// `incident_issues` is keyed on `(incident_id, issue_id, joined_at)`,
	/// so an issue that leaves and rejoins the same incident produces
	/// multiple rows for the same pair. Counts must dedupe on the pair
	/// before joining to `issue_notes`, otherwise every note gets
	/// multiplied by the rejoin count.
	///
	/// Strategy: pull the distinct `(incident_id, issue_id)` pairs first,
	/// then run the per-issue note counts and the direct incident_notes
	/// count concurrently. Takes the pool (`&Db`) so the parallel futures
	/// don't fight over one mutable conn handle.
	pub async fn stats_for(
		pool: &crate::Db,
		incident_ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, IncidentStats>> {
		use crate::schema::{incident_issues, incident_notes, issue_notes};
		use diesel::dsl::count_star;
		use std::collections::{HashMap, HashSet};

		let mut out: HashMap<Uuid, IncidentStats> = incident_ids
			.iter()
			.map(|id| (*id, IncidentStats::default()))
			.collect();
		if incident_ids.is_empty() {
			return Ok(out);
		}

		let ids = incident_ids.to_vec();
		let pairs: Vec<(Uuid, Uuid)> = {
			let mut c = pool.get().await?;
			incident_issues::table
				.filter(incident_issues::incident_id.eq_any(&ids))
				.select((incident_issues::incident_id, incident_issues::issue_id))
				.distinct()
				.load(&mut c)
				.await?
		};

		let mut issues_by_incident: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
		for (incident_id, issue_id) in &pairs {
			issues_by_incident
				.entry(*incident_id)
				.or_default()
				.insert(*issue_id);
		}
		for (incident_id, issues) in &issues_by_incident {
			out.entry(*incident_id).or_default().issue_count = issues.len() as i64;
		}

		let unique_issue_ids: Vec<Uuid> = issues_by_incident
			.values()
			.flatten()
			.copied()
			.collect::<std::collections::HashSet<_>>()
			.into_iter()
			.collect();

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
			if unique_issue_ids.is_empty() {
				return Result::<Vec<(Uuid, i64)>>::Ok(Vec::new());
			}
			let mut c = pool.get().await?;
			issue_notes::table
				.group_by(issue_notes::issue_id)
				.select((issue_notes::issue_id, count_star()))
				.filter(issue_notes::issue_id.eq_any(&unique_issue_ids))
				.load::<(Uuid, i64)>(&mut c)
				.await
				.map_err(AppError::from)
		};
		let (inote_rows, jnote_rows) = futures::try_join!(f_inotes, f_jnotes)?;

		let notes_per_issue: HashMap<Uuid, i64> = jnote_rows.into_iter().collect();

		for (incident_id, issues) in &issues_by_incident {
			let entry = out.entry(*incident_id).or_default();
			for issue_id in issues {
				entry.note_count += notes_per_issue.get(issue_id).copied().unwrap_or(0);
			}
		}
		for (id, n) in inote_rows {
			out.entry(id).or_default().note_count += n;
		}

		Ok(out)
	}
}

impl Incident {
	/// Incidents are owned by the server's group, so a caller asking for a
	/// specific server's incidents really wants the group's. Look up the
	/// server's `group_id` and list incidents on that group; ungrouped
	/// servers return an empty Vec.
	pub async fn list_for_server(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		include_closed: bool,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::incidents::dsl;

		let Some(gid) = Server::get_by_id(db, server_id).await?.group_id else {
			return Ok(Vec::new());
		};
		let mut q = dsl::incidents
			.select(Self::as_select())
			.filter(dsl::server_group_id.eq(gid))
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

	pub async fn list_for_group(
		db: &mut AsyncPgConnection,
		server_group_id: Uuid,
		include_closed: bool,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::incidents::dsl;

		let mut q = dsl::incidents
			.select(Self::as_select())
			.filter(dsl::server_group_id.eq(server_group_id))
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

	/// Incidents that were open at any point at or after `since`: either still
	/// open (`closed_at IS NULL`) or closed no earlier than `since`. Optionally
	/// restricted to one group. Ordered newest-opened first. Drives historical
	/// queries like "incidents open in the past week".
	pub async fn list_open_since(
		db: &mut AsyncPgConnection,
		since: Timestamp,
		group_id: Option<Uuid>,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::incidents::dsl;

		let mut q = dsl::incidents
			.select(Self::as_select())
			.filter(
				dsl::closed_at
					.is_null()
					.or(dsl::closed_at.ge(jiff_diesel::Timestamp::from(since))),
			)
			.into_boxed();
		if let Some(gid) = group_id {
			q = q.filter(dsl::server_group_id.eq(gid));
		}
		q.order(dsl::opened_at.desc())
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

	/// One row per distinct issue. An issue that left and rejoined this
	/// incident shows up once with its most recent link metadata
	/// (`joined_at`/`left_at`); without the dedup a flapping issue can
	/// produce hundreds of repeated rows.
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

		let mut rows: Vec<(IncidentIssue, Issue)> = incident_issues::table
			.inner_join(issues::table.on(issues::id.eq(incident_issues::issue_id)))
			.select((IncidentIssue::as_select(), Issue::as_select()))
			.filter(incident_issues::incident_id.eq(incident_id))
			.distinct_on(incident_issues::issue_id)
			.order((incident_issues::issue_id, incident_issues::joined_at.desc()))
			.load(db)
			.await?;
		rows.sort_by_key(|(link, _)| link.joined_at);
		Ok((incident, rows))
	}

	/// Mark an incident as operator-resolved. This is metadata only — it does
	/// *not* force `closed_at` (that's still driven by auto rules). An
	/// open incident can be resolved (the cause is dealt with) and a closed
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
			let incident_loaded: Incident = incidents::table
				.select(Incident::as_select())
				.filter(incidents::id.eq(incident_id))
				.first(conn)
				.await?;
			let target = IncidentTarget::of_incident(&incident_loaded);

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
				// The issue is now resolved so the leave path fires
				// regardless of `monitored`; pass the live value
				// anyway so the function never sees stale state. Group-scoped
				// issues (server_id = None) bypass the per-server gate.
				let monitored = match issue.server_id {
					Some(sid) => Server::get_by_id(conn, sid).await?.is_monitored,
					None => true,
				};
				re_evaluate_incident_membership(conn, &issue, target, monitored, now, Some(by))
					.await?;
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
