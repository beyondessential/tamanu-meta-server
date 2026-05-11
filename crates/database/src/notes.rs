//! Operator free-text notes attached to issues and incidents.
//!
//! Notes are immutable once written — only `add` and `delete` are supported.
//! Operators who want to "edit" a note should delete it and add a new one;
//! this keeps the schema simple and avoids needing a separate edit log.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::issues::{Incident, Issue};

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(Issue))]
#[diesel(table_name = crate::schema::issue_notes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct IssueNote {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub issue_id: Uuid,
	pub author: String,
	pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(Incident))]
#[diesel(table_name = crate::schema::incident_notes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct IncidentNote {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub incident_id: Uuid,
	pub author: String,
	pub body: String,
}

impl IssueNote {
	pub async fn add(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		author: &str,
		body: &str,
	) -> Result<Self> {
		use crate::schema::issue_notes;
		diesel::insert_into(issue_notes::table)
			.values((
				issue_notes::issue_id.eq(issue_id),
				issue_notes::author.eq(author),
				issue_notes::body.eq(body),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_for_issue(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::issue_notes::dsl;
		dsl::issue_notes
			.select(Self::as_select())
			.filter(dsl::issue_id.eq(issue_id))
			.order(dsl::created_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn delete(db: &mut AsyncPgConnection, note_id: Uuid) -> Result<()> {
		use crate::schema::issue_notes;
		diesel::delete(issue_notes::table.filter(issue_notes::id.eq(note_id)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}
}

impl IncidentNote {
	pub async fn add(
		db: &mut AsyncPgConnection,
		incident_id: Uuid,
		author: &str,
		body: &str,
	) -> Result<Self> {
		use crate::schema::incident_notes;
		diesel::insert_into(incident_notes::table)
			.values((
				incident_notes::incident_id.eq(incident_id),
				incident_notes::author.eq(author),
				incident_notes::body.eq(body),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_for_incident(
		db: &mut AsyncPgConnection,
		incident_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::incident_notes::dsl;
		dsl::incident_notes
			.select(Self::as_select())
			.filter(dsl::incident_id.eq(incident_id))
			.order(dsl::created_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn delete(db: &mut AsyncPgConnection, note_id: Uuid) -> Result<()> {
		use crate::schema::incident_notes;
		diesel::delete(incident_notes::table.filter(incident_notes::id.eq(note_id)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}
}
