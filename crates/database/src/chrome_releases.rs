use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, QueryableByName)]
#[diesel(table_name = crate::schema::chrome_releases)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ChromeRelease {
	pub version: String,
	pub release_date: String,
	pub is_eol: bool,
	pub eol_from: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

#[derive(Debug, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::chrome_releases)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewChromeRelease {
	pub version: String,
	pub release_date: String,
	pub is_eol: bool,
	pub eol_from: Option<String>,
}

impl NewChromeRelease {
	pub async fn save(self, db: &mut AsyncPgConnection) -> Result<ChromeRelease> {
		diesel::insert_into(crate::schema::chrome_releases::table)
			.values(self)
			.get_result(db)
			.await
			.map_err(AppError::from)
	}
}

/// `version` is TEXT holding a Chrome major, so ordering it as text puts
/// `"100"` before `"99"`. Sort on the number it spells instead.
///
/// Anything that isn't a run of digits sorts last (Postgres puts NULLs last on
/// an ascending sort), which matches how the value is read back: a version that
/// won't `parse::<u32>()` is treated as no version at all.
fn version_as_number()
-> diesel::expression::SqlLiteral<diesel::sql_types::Nullable<diesel::sql_types::BigInt>> {
	diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::BigInt>>(
		"case when version ~ '^[0-9]+$' then version::bigint end",
	)
}

impl ChromeRelease {
	pub async fn get_min_version_at_date(
		db: &mut AsyncPgConnection,
		date: Timestamp,
	) -> Result<Option<u32>> {
		use crate::schema::chrome_releases::*;

		let date_str = date.strftime("%Y-%m-%d").to_string();

		// Find minimum version that is released by date and not EOL at that date
		let min_version: Option<String> = table
			.select(version)
			.filter(release_date.le(&date_str))
			.filter(is_eol.eq(false).or(eol_from.gt(&date_str)))
			.order_by(version_as_number().asc())
			.limit(1)
			.first(db)
			.await
			.optional()?;

		Ok(min_version
			.and_then(|v| v.parse::<u32>().ok())
			.map(|v| v.saturating_sub(1)))
	}

	pub async fn delete_all(db: &mut AsyncPgConnection) -> Result<()> {
		use crate::schema::chrome_releases::*;

		diesel::delete(table).execute(db).await?;

		Ok(())
	}

	pub async fn get_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::chrome_releases::*;

		table
			.order_by(version_as_number().asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}
}
