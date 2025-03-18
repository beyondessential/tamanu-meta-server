use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

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

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, QueryableByName)]
#[diesel(table_name = crate::views::version_updates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct VersionUpdate {
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
		major_target: i32,
		minor_target: i32,
		patch_target: i32,
	) -> Vec<Self> {
		use crate::views::version_updates::dsl::*;

		let updates = version_updates
			.filter(major.eq(major_target))
			.filter(
				minor.gt(minor_target).or(
					minor.eq(minor_target).and(patch.gt(patch_target))
				)
			)
			.order_by(minor)
			.select(VersionUpdate::as_select())
			.load(db)
			.await
			.expect("Error loading version updates");

		updates.into_iter().map(|u| Self {
			id: u.id,
			major: u.major,
			minor: u.minor,
			patch: u.patch,
			published: u.published,
			changelog: u.changelog,
		}).collect()
	}
}