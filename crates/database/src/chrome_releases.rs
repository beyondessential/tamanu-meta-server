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

impl ChromeRelease {
	pub async fn get_supported_versions_at_date(
		db: &mut AsyncPgConnection,
		date: Timestamp,
	) -> Result<Vec<u32>> {
		use crate::schema::chrome_releases::*;

		let date_str = date.strftime("%Y-%m-%d").to_string();

		let releases: Vec<ChromeRelease> = table.load(db).await?;

		let supported: Vec<u32> = releases
			.iter()
			.filter_map(|release| {
				let rel_date = &release.release_date;

				if rel_date <= &date_str {
					let eol_date = release.eol_from.as_ref();
					if eol_date.is_none() || eol_date.unwrap() > &date_str {
						release.version.parse::<u32>().ok()
					} else {
						None
					}
				} else {
					None
				}
			})
			.collect();

		Ok(supported)
	}

	pub async fn delete_all(db: &mut AsyncPgConnection) -> Result<()> {
		use crate::schema::chrome_releases::*;

		diesel::delete(table).execute(db).await?;

		Ok(())
	}

	pub async fn get_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::chrome_releases::*;

		table
			.order_by(version.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}
}
