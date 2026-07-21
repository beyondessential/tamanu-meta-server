//! Manual incidents: support-team-recorded incident records, written after
//! the fact rather than derived from check state.
//!
//! Spec: `.workhorse/specs/monitoring/incidents.md` (id `INC`), "Manual
//! incidents". Independent of the issue/incident machinery in
//! [`crate::issues`]: nothing joins these, they never notify, and only the
//! people editing them change them. Written over the MCP interface (see
//! `.workhorse/specs/private-server/mcp.md`), displayed read-only in the
//! operator UI.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::manual_incidents)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ManualIncident {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// Single-line headline.
	pub title: String,
	/// Markdown body; empty when nobody has written one yet.
	pub description: String,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub started_at: Timestamp,
	/// `None` while the incident is ongoing.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub ended_at: Option<Timestamp>,
	/// The affected server group, or `None` for an incident concerning the
	/// fleet or Canopy generally.
	pub server_group_id: Option<Uuid>,
	/// Who recorded it: a tailnet login or an MCP token name.
	pub created_by: String,
}

/// Field edits for [`ManualIncident::update`]. `None` leaves a field alone;
/// `ended_at` uses a double `Option` so `Some(None)` explicitly clears the
/// end time (marking the incident ongoing again).
#[derive(Clone, Debug, Default)]
pub struct ManualIncidentUpdate {
	pub title: Option<String>,
	pub description: Option<String>,
	pub started_at: Option<Timestamp>,
	pub ended_at: Option<Option<Timestamp>>,
}

impl ManualIncident {
	pub async fn create(
		db: &mut AsyncPgConnection,
		title: &str,
		description: &str,
		started_at: Timestamp,
		ended_at: Option<Timestamp>,
		server_group_id: Option<Uuid>,
		created_by: &str,
	) -> Result<Self> {
		use crate::schema::manual_incidents::dsl;

		diesel::insert_into(dsl::manual_incidents)
			.values((
				dsl::title.eq(title),
				dsl::description.eq(description),
				dsl::started_at.eq(jiff_diesel::Timestamp::from(started_at)),
				dsl::ended_at.eq(jiff_diesel::NullableTimestamp::from(ended_at)),
				dsl::server_group_id.eq(server_group_id),
				dsl::created_by.eq(created_by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get(db: &mut AsyncPgConnection, id: Uuid) -> Result<Option<Self>> {
		use crate::schema::manual_incidents::dsl;

		dsl::manual_incidents
			.select(Self::as_select())
			.filter(dsl::id.eq(id))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Like [`Self::get`], but an unknown id errors (404).
	pub async fn get_required(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		Self::get(db, id)
			.await?
			.ok_or_else(|| diesel::result::Error::NotFound.into())
	}

	/// Most recently started first. `group_id` narrows to one group's
	/// incidents; `ongoing_only` keeps only those without an end time.
	pub async fn list(
		db: &mut AsyncPgConnection,
		group_id: Option<Uuid>,
		ongoing_only: bool,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::manual_incidents::dsl;

		let mut query = dsl::manual_incidents
			.select(Self::as_select())
			.order(dsl::started_at.desc())
			.limit(limit)
			.into_boxed();
		if let Some(group_id) = group_id {
			query = query.filter(dsl::server_group_id.eq(group_id));
		}
		if ongoing_only {
			query = query.filter(dsl::ended_at.is_null());
		}
		query.load(db).await.map_err(AppError::from)
	}

	/// Apply the given edits. `None` for an unknown id.
	pub async fn update(
		db: &mut AsyncPgConnection,
		id: Uuid,
		up: ManualIncidentUpdate,
	) -> Result<Option<Self>> {
		use crate::schema::manual_incidents::dsl;

		let Some(current) = Self::get(db, id).await? else {
			return Ok(None);
		};
		let title = up.title.unwrap_or(current.title);
		let description = up.description.unwrap_or(current.description);
		let started_at = up.started_at.unwrap_or(current.started_at);
		let ended_at = match up.ended_at {
			Some(ended_at) => ended_at,
			None => current.ended_at,
		};

		diesel::update(dsl::manual_incidents.filter(dsl::id.eq(id)))
			.set((
				dsl::title.eq(title),
				dsl::description.eq(description),
				dsl::started_at.eq(jiff_diesel::Timestamp::from(started_at)),
				dsl::ended_at.eq(jiff_diesel::NullableTimestamp::from(ended_at)),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Remove the record. `false` for an unknown id.
	pub async fn delete(db: &mut AsyncPgConnection, id: Uuid) -> Result<bool> {
		use crate::schema::manual_incidents::dsl;

		let affected = diesel::delete(dsl::manual_incidents.filter(dsl::id.eq(id)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(affected > 0)
	}
}
