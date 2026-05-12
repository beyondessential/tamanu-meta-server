//! Slack delivery outbox.
//!
//! Each row is one Slack message we owe somebody. State-change call sites
//! ([`Incident::open_for`](crate::issues::Incident::open_for),
//! [`Incident::resolve`](crate::issues::Incident::resolve), and — in Phase B
//! — issue join/leave and `IncidentNote::add`) insert a row inside their own
//! transaction. The `slacker_outbox` job binary drains the table, posts to
//! Slack, and stamps `delivered_at` on success.
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
}

impl SlackOutbox {
	/// Insert a new outbox row. Caller is expected to be inside the
	/// transaction that's mutating the underlying incident/issue/note so
	/// the outbox row and the state change commit (or roll back) together.
	pub async fn enqueue(
		db: &mut AsyncPgConnection,
		kind: &str,
		incident_id: Uuid,
		issue_id: Option<Uuid>,
		note_id: Option<Uuid>,
		payload: JsonValue,
	) -> Result<Self> {
		use crate::schema::slack_outbox;
		diesel::insert_into(slack_outbox::table)
			.values((
				slack_outbox::kind.eq(kind),
				slack_outbox::incident_id.eq(incident_id),
				slack_outbox::issue_id.eq(issue_id),
				slack_outbox::note_id.eq(note_id),
				slack_outbox::payload.eq(payload),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Claim up to `limit` pending rows in insertion order. Uses
	/// `FOR UPDATE SKIP LOCKED` so multiple workers (or a worker plus a
	/// hand-run reprocess) won't fight over the same row. The caller must
	/// hold these inside its own transaction and call
	/// [`mark_delivered`](Self::mark_delivered) or
	/// [`mark_failed`](Self::mark_failed) before committing.
	pub async fn claim_pending(
		db: &mut AsyncPgConnection,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::slack_outbox::dsl;
		dsl::slack_outbox
			.select(Self::as_select())
			.filter(dsl::delivered_at.is_null())
			.order(dsl::created_at.asc())
			.limit(limit)
			.for_update()
			.skip_locked()
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn mark_delivered(db: &mut AsyncPgConnection, id: Uuid) -> Result<()> {
		use crate::schema::slack_outbox::dsl;
		diesel::update(dsl::slack_outbox.filter(dsl::id.eq(id)))
			.set((
				dsl::delivered_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				dsl::last_error.eq(None::<String>),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	pub async fn mark_failed(
		db: &mut AsyncPgConnection,
		id: Uuid,
		error: &str,
	) -> Result<()> {
		use crate::schema::slack_outbox::dsl;
		diesel::update(dsl::slack_outbox.filter(dsl::id.eq(id)))
			.set((
				dsl::attempts.eq(dsl::attempts + 1),
				dsl::last_error.eq(error),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}
}
