use commons_errors::{AppError, Result};
use commons_types::version::{VersionStatus, VersionStr};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[macro_export]
macro_rules! predicate_version {
	($version:expr) => {{
		use ::diesel::BoolExpressionMethods as _;
		use $crate::schema::versions::dsl::*;
		let ::node_semver::Version {
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
pub use predicate_version;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, QueryableByName)]
#[diesel(table_name = crate::schema::versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Version {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub status: VersionStatus,
	pub changelog: String,
	pub device_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, QueryableByName)]
#[diesel(table_name = crate::views::version_updates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ViewVersion {
	pub id: Uuid,
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub status: VersionStatus,
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
	pub status: VersionStatus,
	pub device_id: Option<Uuid>,
}

impl Version {
	pub fn as_semver(&self) -> node_semver::Version {
		node_semver::Version::new(self.major as _, self.minor as _, self.patch as _)
	}

	pub async fn get_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::versions::*;

		table
			.select(Version::as_select())
			.filter(status.eq(VersionStatus::Published))
			.order_by(major.desc())
			.then_order_by(minor.desc())
			.then_order_by(patch.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_all_including_drafts(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::versions::*;

		table
			.select(Version::as_select())
			.order_by(major.desc())
			.then_order_by(minor.desc())
			.then_order_by(patch.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_version(db: &mut AsyncPgConnection, version: VersionStr) -> Result<Self> {
		use crate::schema::versions::*;

		table
			.filter(predicate_version!(version.0))
			.select(Version::as_select())
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_updates_for_version(
		db: &mut AsyncPgConnection,
		version: VersionStr,
	) -> Result<Vec<ViewVersion>> {
		use crate::views::version_updates::dsl::*;
		let node_semver::Version {
			major: target_major,
			minor: target_minor,
			patch: target_patch,
			..
		} = version.0;
		version_updates
			.filter(
				major
					.eq(target_major as i32)
					.and(status.eq(VersionStatus::Published))
					.and(
						minor.gt(target_minor as i32).or(minor
							.eq(target_minor as i32)
							.and(patch.gt(target_patch as i32))),
					),
			)
			.order_by(minor)
			.select(version_updates::all_columns())
			.load(db)
			.await
			.map_err(AppError::from)
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
				status
					.eq(VersionStatus::Published)
					.and(major.ge(target_major as i32))
					.and(minor.ge(target_minor as i32))
					.and(patch.ge(target_patch as i32)),
			)
			.order_by(major.desc())
			.then_order_by(minor.desc())
			.then_order_by(patch.desc())
			.load(db)
			.await
			.map_err(AppError::from)?
			.into_iter()
			.find(|v| range.satisfies(&v.as_semver()))
			.ok_or(AppError::NoMatchingVersions)
	}

	pub async fn get_head_release_date(
		db: &mut AsyncPgConnection,
		version: VersionStr,
	) -> Result<Timestamp> {
		use crate::schema::versions::*;

		let node_semver::Version {
			major: target_major,
			minor: target_minor,
			..
		} = version.0;

		table
			.select(Version::as_select())
			.filter(
				major
					.eq(target_major as i32)
					.and(minor.eq(target_minor as i32))
					.and(patch.eq(0)),
			)
			.first(db)
			.await
			.map(|v: Version| v.created_at)
			.map_err(AppError::from)
	}

	pub async fn update_status(
		db: &mut AsyncPgConnection,
		version: VersionStr,
		new_status: VersionStatus,
	) -> Result<()> {
		use crate::schema::versions::dsl::*;

		diesel::update(versions)
			.filter(predicate_version!(version.0))
			.set(status.eq(new_status))
			.execute(db)
			.await?;

		Ok(())
	}

	pub async fn update_changelog(
		db: &mut AsyncPgConnection,
		version: VersionStr,
		new_changelog: String,
	) -> Result<()> {
		use crate::schema::versions::dsl::*;

		diesel::update(versions)
			.filter(predicate_version!(version.0))
			.set(changelog.eq(new_changelog))
			.execute(db)
			.await?;

		Ok(())
	}

	pub async fn update_device_id(
		db: &mut AsyncPgConnection,
		version: VersionStr,
		new_device_id: Uuid,
	) -> Result<()> {
		use crate::schema::versions::dsl::*;

		diesel::update(versions)
			.filter(predicate_version!(version.0))
			.set(device_id.eq(new_device_id))
			.execute(db)
			.await?;

		Ok(())
	}

	pub async fn is_latest_in_minor(
		db: &mut AsyncPgConnection,
		version: VersionStr,
	) -> Result<bool> {
		use crate::schema::versions::dsl::*;

		let version_record = Self::get_by_version(db, version.clone()).await?;

		let latest_in_minor: Option<Version> = versions
			.filter(major.eq(version_record.major))
			.filter(minor.eq(version_record.minor))
			.filter(status.eq(VersionStatus::Published))
			.order_by(patch.desc())
			.select(Version::as_select())
			.first(db)
			.await
			.ok();

		Ok(latest_in_minor
			.as_ref()
			.map(|v| v.patch == version_record.patch)
			.unwrap_or(true))
	}

	pub async fn get_all_in_minor(
		db: &mut AsyncPgConnection,
		version: VersionStr,
	) -> Result<Vec<Self>> {
		use crate::schema::versions::dsl::*;

		let node_semver::Version {
			major: target_major,
			minor: target_minor,
			patch: target_patch,
			..
		} = version.0;

		versions
			.filter(major.eq(target_major as i32))
			.filter(minor.eq(target_minor as i32))
			.filter(patch.lt(target_patch as i32))
			.filter(status.ne(VersionStatus::Draft))
			.order_by(patch.desc())
			.select(Version::as_select())
			.load(db)
			.await
			.map_err(AppError::from)
	}
}
