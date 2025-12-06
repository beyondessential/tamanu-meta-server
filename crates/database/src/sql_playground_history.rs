use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::sql_playground_history)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SqlPlaygroundHistory {
	/// The ID of the history entry.
	pub id: Uuid,

	/// The SQL query that was executed.
	pub query: String,

	/// The Tailscale user who ran the query.
	pub tailscale_user: String,

	/// The created timestamp.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::sql_playground_history)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewSqlPlaygroundHistory {
	/// The SQL query that was executed.
	pub query: String,

	/// The Tailscale user who ran the query.
	pub tailscale_user: String,
}

impl SqlPlaygroundHistory {
	/// Create a new history entry.
	pub async fn create(
		db: &mut AsyncPgConnection,
		query: String,
		tailscale_user: String,
	) -> Result<Self> {
		use crate::schema::sql_playground_history::dsl;

		let new_entry = NewSqlPlaygroundHistory {
			query,
			tailscale_user,
		};

		diesel::insert_into(dsl::sql_playground_history)
			.values(&new_entry)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Get the last query executed by a specific user.
	pub async fn get_last_by_user(
		db: &mut AsyncPgConnection,
		tailscale_user: &str,
	) -> Result<Option<Self>> {
		use crate::schema::sql_playground_history::dsl;

		dsl::sql_playground_history
			.filter(dsl::tailscale_user.eq(tailscale_user))
			.order(dsl::created_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Get paginated history for all users, ordered by created_at descending.
	pub async fn get_paginated(
		db: &mut AsyncPgConnection,
		offset: i64,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::sql_playground_history::dsl;

		dsl::sql_playground_history
			.order(dsl::created_at.desc())
			.offset(offset)
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Count total history entries.
	pub async fn count_all(db: &mut AsyncPgConnection) -> Result<i64> {
		use crate::schema::sql_playground_history::dsl;
		use diesel::dsl::count_star;

		dsl::sql_playground_history
			.select(count_star())
			.first(db)
			.await
			.map_err(AppError::from)
	}
}
