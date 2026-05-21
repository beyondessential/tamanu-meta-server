//! Operator-flagged known issues attached to a version range.
//!
//! Each row covers a half-open range `[min, max)` of patches within a
//! single minor branch. Raising sets `min` to the affected version and
//! leaves `max_*` NULL — the issue then implicitly covers every later
//! patch in that minor. Resolving sets `max_*` to the fix version (the
//! first unaffected patch). Resolution is append-only: instead of
//! editing, an operator records a `resolution_message` alongside the
//! fix version.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::version_known_issues)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct VersionKnownIssue {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub author: String,
	pub description: String,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolution_message: Option<String>,
	pub min_major: i32,
	pub min_minor: i32,
	pub min_patch: i32,
	pub max_major: Option<i32>,
	pub max_minor: Option<i32>,
	pub max_patch: Option<i32>,
}

impl VersionKnownIssue {
	pub async fn add(
		db: &mut AsyncPgConnection,
		min: (i32, i32, i32),
		author: &str,
		description: &str,
	) -> Result<Self> {
		use crate::schema::version_known_issues;
		diesel::insert_into(version_known_issues::table)
			.values((
				version_known_issues::min_major.eq(min.0),
				version_known_issues::min_minor.eq(min.1),
				version_known_issues::min_patch.eq(min.2),
				version_known_issues::author.eq(author),
				version_known_issues::description.eq(description),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// All known issues whose minor branch matches the given (major, minor)
	/// — ordered newest first. Used by the version-detail UI to show
	/// every issue ever raised against this minor, including ones already
	/// resolved on an earlier patch.
	pub async fn list_for_minor(
		db: &mut AsyncPgConnection,
		major: i32,
		minor: i32,
	) -> Result<Vec<Self>> {
		use crate::schema::version_known_issues::dsl;
		dsl::version_known_issues
			.select(Self::as_select())
			.filter(dsl::min_major.eq(major))
			.filter(dsl::min_minor.eq(minor))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn resolve(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		fix: (i32, i32, i32),
		resolved_by: &str,
		resolution_message: &str,
	) -> Result<Self> {
		use crate::schema::version_known_issues::dsl;
		let now = Timestamp::now();
		diesel::update(dsl::version_known_issues)
			.filter(dsl::id.eq(issue_id))
			.filter(dsl::max_major.is_null())
			.filter(dsl::min_major.eq(fix.0))
			.filter(dsl::min_minor.eq(fix.1))
			.filter(dsl::min_patch.lt(fix.2))
			.set((
				dsl::max_major.eq(Some(fix.0)),
				dsl::max_minor.eq(Some(fix.1)),
				dsl::max_patch.eq(Some(fix.2)),
				dsl::resolved_at.eq(jiff_diesel::NullableTimestamp::from(Some(now))),
				dsl::resolved_by.eq(resolved_by),
				dsl::resolution_message.eq(resolution_message),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Subset of `ids` whose (major, minor, patch) is covered by any
	/// known issue's range. Used to compute the `ready` flag in batch.
	pub async fn affected_versions(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashSet<Uuid>> {
		use crate::schema::{version_known_issues as k, versions as v};
		if ids.is_empty() {
			return Ok(std::collections::HashSet::new());
		}
		let rows: Vec<Uuid> = v::table
			.inner_join(
				k::table.on(k::min_major
					.eq(v::major)
					.and(k::min_minor.eq(v::minor))
					.and(v::patch.ge(k::min_patch))
					.and(k::max_patch.is_null().or(v::patch.lt(k::max_patch.assume_not_null())))),
			)
			.select(v::id)
			.filter(v::id.eq_any(ids))
			.distinct()
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows.into_iter().collect())
	}

	/// Whether a specific version (by coordinates) is unaffected by any
	/// known issue.
	pub async fn version_is_ready(
		db: &mut AsyncPgConnection,
		major: i32,
		minor: i32,
		patch: i32,
	) -> Result<bool> {
		use crate::schema::version_known_issues::dsl;
		let count: i64 = dsl::version_known_issues
			.filter(dsl::min_major.eq(major))
			.filter(dsl::min_minor.eq(minor))
			.filter(dsl::min_patch.le(patch))
			.filter(
				dsl::max_patch
					.is_null()
					.or(dsl::max_patch.assume_not_null().gt(patch)),
			)
			.count()
			.get_result(db)
			.await
			.map_err(AppError::from)?;
		Ok(count == 0)
	}
}
