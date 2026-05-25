//! Slack delivery outbox.
//!
//! Each row is one Slack message we owe somebody. State-change call sites
//! ([`Incident::open_for`](crate::issues::Incident::open_for),
//! [`Incident::resolve`](crate::issues::Incident::resolve), and — in Phase B
//! — issue join/leave and `IncidentNote::add`) insert a row inside their own
//! transaction. The `slacker_outbox` job binary drains the table, posts to
//! Slack, and stamps `delivered_at` on success.
//!
//! Each row carries a `deliver_after` timestamp. Resolves enqueue with
//! `deliver_after = now`, so they ship on the next drain tick. Opens enqueue
//! with a small future delay (the per-group `slack_open_delay`) so a probe
//! that flaps open and resolved within that window can be cancelled before
//! we tell Slack anything happened: the cascade in
//! [`crate::issues::Incident::resolve`] and the auto-close path both call
//! [`SlackOutbox::cancel_pending_open`] before enqueueing the resolve,
//! which marks the open row given-up and skips the resolve when it
//! succeeds.
//!
//! The payload is rendered to Block Kit JSON at enqueue time, not at delivery
//! time, so we capture state as it was when the event happened rather than
//! risk reading a later (resolved / different-severity) state when the worker
//! eventually wakes up.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

pub mod vars;

/// Newly-opened incident — top-level message.
pub const KIND_INCIDENT_OPEN: &str = "incident_open";
/// Incident resolved — Phase A: top-level; Phase B: reply in the incident thread.
pub const KIND_INCIDENT_RESOLVE: &str = "incident_resolve";

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::slack_outbox)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SlackOutbox {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub kind: String,
	pub incident_id: Uuid,
	pub issue_id: Option<Uuid>,
	pub note_id: Option<Uuid>,
	pub payload: JsonValue,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub delivered_at: Option<Timestamp>,
	pub attempts: i32,
	pub last_error: Option<String>,
	/// Last raw HTTP body Slack returned (whether 2xx or not). Slack
	/// Workflow Builder webhooks 2xx the *trigger acceptance* even when
	/// the workflow downstream did nothing, so a delivered row with an
	/// unexpected body is the only DB-side evidence of that failure mode.
	pub last_response: Option<String>,
	/// Set when the drainer has stopped retrying this row (max attempts
	/// hit). Distinct from `delivered_at` — gave-up rows never reached
	/// Slack. The pending index excludes both, so `claim_pending` won't
	/// pick them up again.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub gave_up_at: Option<Timestamp>,
	/// Earliest time `claim_pending` will return this row. Resolves are
	/// enqueued with `deliver_after = now` (ship immediately); opens use
	/// `now + group.slack_open_delay` so flap-and-resolve incidents can be
	/// cancelled before they hit Slack.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub deliver_after: Timestamp,
}

impl SlackOutbox {
	/// Insert a new outbox row. Caller is expected to be inside the
	/// transaction that's mutating the underlying incident/issue/note so
	/// the outbox row and the state change commit (or roll back) together.
	///
	/// `deliver_after` is the earliest time the drainer is allowed to ship
	/// this row. Most callers pass `Timestamp::now()`; the open path
	/// passes `now + group.slack_open_delay`.
	pub async fn enqueue(
		db: &mut AsyncPgConnection,
		kind: &str,
		incident_id: Uuid,
		issue_id: Option<Uuid>,
		note_id: Option<Uuid>,
		payload: JsonValue,
		deliver_after: Timestamp,
	) -> Result<Self> {
		use crate::schema::slack_outbox;
		diesel::insert_into(slack_outbox::table)
			.values((
				slack_outbox::kind.eq(kind),
				slack_outbox::incident_id.eq(incident_id),
				slack_outbox::issue_id.eq(issue_id),
				slack_outbox::note_id.eq(note_id),
				slack_outbox::payload.eq(payload),
				slack_outbox::deliver_after.eq(jiff_diesel::Timestamp::from(deliver_after)),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Mark every still-pending `incident_open` row for this incident as
	/// given-up, with a reason that documents why the cancellation
	/// happened. Returns the number of rows affected — a non-zero result
	/// means the open never reached Slack and the caller should skip the
	/// resolve enqueue too (we don't want to "resolve" something Slack
	/// never heard about). Rows the drainer has already delivered, or has
	/// already given up on, are untouched.
	pub async fn cancel_pending_open(
		db: &mut AsyncPgConnection,
		incident_id: Uuid,
		reason: &str,
	) -> Result<usize> {
		use crate::schema::slack_outbox::dsl;
		let now = Timestamp::now();
		let rows = diesel::update(
			dsl::slack_outbox
				.filter(dsl::incident_id.eq(incident_id))
				.filter(dsl::kind.eq(KIND_INCIDENT_OPEN))
				.filter(dsl::delivered_at.is_null())
				.filter(dsl::gave_up_at.is_null()),
		)
		.set((
			dsl::gave_up_at.eq(jiff_diesel::Timestamp::from(now)),
			dsl::last_error.eq(reason),
		))
		.execute(db)
		.await
		.map_err(AppError::from)?;
		Ok(rows)
	}

	/// Claim up to `limit` pending rows in insertion order. Uses
	/// `FOR UPDATE SKIP LOCKED` so multiple workers (or a worker plus a
	/// hand-run reprocess) won't fight over the same row. The caller must
	/// hold these inside its own transaction and call
	/// [`mark_delivered`](Self::mark_delivered),
	/// [`mark_failed`](Self::mark_failed), or
	/// [`mark_given_up`](Self::mark_given_up) before committing.
	pub async fn claim_pending(db: &mut AsyncPgConnection, limit: i64) -> Result<Vec<Self>> {
		use crate::schema::slack_outbox::dsl;
		let now = Timestamp::now();
		dsl::slack_outbox
			.select(Self::as_select())
			.filter(dsl::delivered_at.is_null())
			.filter(dsl::gave_up_at.is_null())
			.filter(dsl::deliver_after.le(jiff_diesel::Timestamp::from(now)))
			.order(dsl::created_at.asc())
			.limit(limit)
			.for_update()
			.skip_locked()
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// `response` is the raw HTTP body Slack returned (may be empty). It's
	/// recorded so postmortems can tell a real delivery apart from
	/// "Slack 2xx'd the trigger and the workflow did nothing".
	pub async fn mark_delivered(
		db: &mut AsyncPgConnection,
		id: Uuid,
		response: &str,
	) -> Result<()> {
		use crate::schema::slack_outbox::dsl;
		diesel::update(dsl::slack_outbox.filter(dsl::id.eq(id)))
			.set((
				dsl::delivered_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				dsl::last_error.eq(None::<String>),
				dsl::last_response.eq(response),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// `response` is the HTTP body Slack returned for this attempt, if we
	/// got one (network errors before any response leave this `None`).
	pub async fn mark_failed(
		db: &mut AsyncPgConnection,
		id: Uuid,
		error: &str,
		response: Option<&str>,
	) -> Result<()> {
		use crate::schema::slack_outbox::dsl;
		diesel::update(dsl::slack_outbox.filter(dsl::id.eq(id)))
			.set((
				dsl::attempts.eq(dsl::attempts + 1),
				dsl::last_error.eq(error),
				dsl::last_response.eq(response),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Mark the row as terminally given-up — the drainer will not retry
	/// it. Sets `gave_up_at` so the pending index excludes it. Leaves
	/// `last_error` / `last_response` as the operator-visible record of
	/// why we stopped.
	pub async fn mark_given_up(db: &mut AsyncPgConnection, id: Uuid, error: &str) -> Result<()> {
		use crate::schema::slack_outbox::dsl;
		diesel::update(dsl::slack_outbox.filter(dsl::id.eq(id)))
			.set((
				dsl::gave_up_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				dsl::last_error.eq(error),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}
}
