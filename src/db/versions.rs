use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{AsyncPgConnection, prelude::*};
use uuid::Uuid;

use crate::{
	error::{AppError, Result},
	servers::version::Version as ParsedVersion,
};

#[macro_export]
macro_rules! predicate_version {
	($version:expr) => {{
		use $crate::schema::versions::dsl::*;
		let node_semver::Version {
			major: target_major,
			minor: target_minor,
			patch: target_patch,
			..
		} = $version;

		major
			.eq(target_major as i32)
			.and(minor.eq(target_minor as i32))
			.and(patch.eq(target_patch as i32))
	}};
}
pub(crate) use predicate_version;

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, QueryableByName,
)]
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

#[derive(Debug, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewVersion {
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub changelog: String,
}

impl Version {
	pub fn as_semver(&self) -> node_semver::Version {
		node_semver::Version::new(self.major as _, self.minor as _, self.patch as _)
	}

	pub async fn get_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::versions::*;

		table
			.select(Version::as_select())
			.order_by(major.desc())
			.then_order_by(minor.desc())
			.then_order_by(patch.desc())
			.load(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub async fn get_by_version(
		db: &mut AsyncPgConnection,
		version: ParsedVersion,
	) -> Result<Self> {
		use crate::schema::versions::*;

		table
			.filter(predicate_version!(version.0))
			.select(Version::as_select())
			.first(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub async fn get_updates_for_version(
		db: &mut AsyncPgConnection,
		version: ParsedVersion,
	) -> Result<Vec<Self>> {
		use crate::views::version_updates::dsl::*;
		let node_semver::Version {
			major: target_major,
			minor: target_minor,
			patch: target_patch,
			..
		} = version.0;
		version_updates
			.filter(
				major.eq(target_major as i32).and(published.eq(true)).and(
					minor.gt(target_minor as i32).or(minor
						.eq(target_minor as i32)
						.and(patch.gt(target_patch as i32))),
				),
			)
			.order_by(minor)
			.select(version_updates::all_columns())
			.load(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub async fn get_latest_matching(
		db: &mut AsyncPgConnection,
		range: node_semver::Range,
	) -> Result<Self> {
		use crate::schema::versions::*;

		let node_semver::Version {
			major: target_major,
			minor: target_minor,
			patch: target_patch,
			..
		} = range.min_version().ok_or(AppError::UnusableRange)?;

		table
			.select(Version::as_select())
			.filter(
				published
					.eq(true)
					.and(major.ge(target_major as i32))
					.and(minor.ge(target_minor as i32))
					.and(patch.ge(target_patch as i32)),
			)
			.order_by(major.desc())
			.then_order_by(minor.desc())
			.then_order_by(patch.desc())
			.load(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))?
			.into_iter()
			.find(|v| range.satisfies(&v.as_semver()))
			.ok_or(AppError::NoMatchingVersions)
	}
}
