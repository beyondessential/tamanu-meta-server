use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;
use crate::app::Version as ParsedVersion;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, QueryableByName)]
#[diesel(table_name = crate::schema::versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Version {
	pub id: Uuid,
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub published: bool,
	pub changelog: String,
}

impl Version {
	pub async fn get_all(db: &mut AsyncPgConnection) -> Vec<Self> {
		use crate::schema::versions::*;

		table
			.select(Version::as_select())
			.order_by(major.desc())
			.then_order_by(minor.desc())
			.then_order_by(patch.desc())
			.load(db)
			.await
			.expect("Error loading versions")
	}

	pub async fn get_updates_for_version(
		db: &mut AsyncPgConnection,
		version: ParsedVersion,
	) -> Vec<Self> {
		use crate::views::version_updates::dsl::*;
		let minor_target: i32 = version.0.minor as i32;
		let major_target: i32 = version.0.major as i32;
		let patch_target: i32 = version.0.patch as i32;
		version_updates
			.filter(major.eq(major_target))
			.filter(published.eq(true))
			.filter(
				minor.gt(minor_target).or(
					minor.eq(minor_target).and(patch.gt(patch_target))
				)
			)
			.order_by(minor)
			.select(version_updates::all_columns())
			.load(db)
			.await
			.expect("Error loading version updates")
	}
}