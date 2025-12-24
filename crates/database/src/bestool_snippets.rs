use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::bestool_snippets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BestoolSnippet {
	/// The unique ID of the snippet.
	pub id: Uuid,

	/// When the snippet was created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,

	/// When the snippet was soft-deleted, if at all.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub deleted_at: Option<Timestamp>,

	/// The ID of the snippet this one supersedes (creates a version chain).
	pub supersedes_id: Option<Uuid>,

	/// The user who created the snippet.
	pub editor: String,

	/// The name of the snippet.
	pub name: String,

	/// Optional description of the snippet.
	pub description: Option<String>,

	/// The SQL query content.
	pub sql: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::bestool_snippets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewBestoolSnippet {
	/// The user who is creating the snippet.
	pub editor: String,

	/// The name of the snippet.
	pub name: String,

	/// Optional description of the snippet.
	pub description: Option<String>,

	/// The SQL query content.
	pub sql: String,

	/// Optional reference to the snippet being superseded.
	pub supersedes_id: Option<Uuid>,
}

impl BestoolSnippet {
	/// Create a new snippet.
	pub async fn create(
		db: &mut AsyncPgConnection,
		editor: String,
		name: String,
		description: Option<String>,
		sql: String,
		supersedes_id: Option<Uuid>,
	) -> Result<Self> {
		use crate::schema::bestool_snippets::dsl;

		let new_snippet = NewBestoolSnippet {
			editor,
			name,
			description,
			sql,
			supersedes_id,
		};

		diesel::insert_into(dsl::bestool_snippets)
			.values(&new_snippet)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Get a snippet by ID.
	pub async fn get_by_id(db: &mut AsyncPgConnection, id: Uuid) -> Result<Option<Self>> {
		use crate::schema::bestool_snippets::dsl;

		dsl::bestool_snippets
			.filter(dsl::id.eq(id))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Get an active snippet by name (where supersedes_id and deleted_at are NULL).
	pub async fn get_active_by_name(
		db: &mut AsyncPgConnection,
		name: &str,
	) -> Result<Option<Self>> {
		use crate::schema::bestool_snippets::dsl;

		dsl::bestool_snippets
			.filter(dsl::name.eq(name))
			.filter(dsl::supersedes_id.is_null())
			.filter(dsl::deleted_at.is_null())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Get all active snippets (where supersedes_id and deleted_at are NULL).
	pub async fn get_all_active(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::bestool_snippets::dsl;

		dsl::bestool_snippets
			.filter(dsl::supersedes_id.is_null())
			.filter(dsl::deleted_at.is_null())
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Get the version history of a snippet (all versions including deleted/superseded).
	pub async fn get_version_history(db: &mut AsyncPgConnection, name: &str) -> Result<Vec<Self>> {
		use crate::schema::bestool_snippets::dsl;

		dsl::bestool_snippets
			.filter(dsl::name.eq(name))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Soft-delete a snippet by setting its deleted_at timestamp.
	pub async fn delete(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		use crate::schema::bestool_snippets::dsl;
		use diesel::dsl::now;

		diesel::update(dsl::bestool_snippets.filter(dsl::id.eq(id)))
			.set(dsl::deleted_at.eq(now))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Get the edit history of this entry (all versions that led to this entry).
	///
	/// Does not include the current entry.
	pub async fn get_edit_history(&self, db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		let mut history = Vec::new();
		let mut current_id = self.supersedes_id;

		while let Some(prev_id) = current_id {
			if let Some(prev) = Self::get_by_id(db, prev_id).await? {
				current_id = prev.supersedes_id;
				history.push(prev);
			} else {
				break;
			}
		}

		Ok(history)
	}

	/// Count active snippets (where supersedes_id and deleted_at are NULL).
	pub async fn count_current(db: &mut AsyncPgConnection) -> Result<i64> {
		use crate::schema::bestool_snippets::dsl;
		use diesel::dsl::count_star;

		let superseded_subquery = dsl::bestool_snippets
			.filter(dsl::supersedes_id.is_not_null())
			.select(dsl::supersedes_id)
			.into_boxed();

		dsl::bestool_snippets
			.filter(dsl::deleted_at.is_null())
			.filter(dsl::id.nullable().ne_all(superseded_subquery))
			.select(count_star())
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_current(
		db: &mut AsyncPgConnection,
		offset: i64,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::bestool_snippets::dsl;

		let superseded_subquery = dsl::bestool_snippets
			.filter(dsl::supersedes_id.is_not_null())
			.select(dsl::supersedes_id)
			.into_boxed();

		dsl::bestool_snippets
			.filter(dsl::deleted_at.is_null())
			.filter(dsl::id.nullable().ne_all(superseded_subquery))
			.order(dsl::name)
			.offset(offset)
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Get the latest version of a snippet (following the supersedes chain forward).
	/// Returns the same snippet ID if it's already the latest.
	pub async fn get_latest_id(db: &mut AsyncPgConnection, mut id: Uuid) -> Result<Uuid> {
		use crate::schema::bestool_snippets::dsl;

		// Keep finding snippets that supersede this one until we reach the latest
		loop {
			let next_version: Option<Uuid> = dsl::bestool_snippets
				.filter(dsl::supersedes_id.eq(id))
				.select(dsl::id)
				.first(db)
				.await
				.optional()?;

			match next_version {
				Some(new_id) => id = new_id,
				None => return Ok(id),
			}
		}
	}
}
