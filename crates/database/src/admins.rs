use commons_errors::{AppError, Result};
use diesel::{dsl::count, prelude::*};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::admins)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Admin {
	pub email: String,
	pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Admin {
	pub async fn check_email(db: &mut AsyncPgConnection, email: &str) -> Result<bool> {
		use crate::schema::admins::dsl;
		dsl::admins
			.select(count(dsl::email))
			.filter(dsl::email.eq(email))
			.first(db)
			.await
			.map_err(AppError::from)
			.map(|count: i64| count > 0)
	}

	pub async fn add(db: &mut AsyncPgConnection, email: &str) -> Result<Self> {
		use crate::schema::admins::dsl;
		diesel::insert_into(dsl::admins)
			.values(dsl::email.eq(email))
			.on_conflict_do_nothing()
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn delete(db: &mut AsyncPgConnection, email: &str) -> Result<()> {
		use crate::schema::admins::dsl;
		diesel::delete(dsl::admins)
			.filter(dsl::email.eq(email))
			.execute(db)
			.await
			.map_err(AppError::from)
			.map(|_| ())
	}
}
