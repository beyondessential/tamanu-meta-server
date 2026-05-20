//! Operator-flagged known issues attached to a version.
//!
//! Rows are append-only: instead of editing or deleting, an operator
//! resolves an issue with a `resolution_message`. A version is `ready`
//! when it has no unresolved (open) known issues.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::versions::Version;

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(Version))]
#[diesel(table_name = crate::schema::version_known_issues)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct VersionKnownIssue {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub version_id: Uuid,
	pub author: String,
	pub description: String,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolution_message: Option<String>,
}

impl VersionKnownIssue {
	pub async fn add(
		db: &mut AsyncPgConnection,
		version_id: Uuid,
		author: &str,
		description: &str,
	) -> Result<Self> {
		use crate::schema::version_known_issues;
		diesel::insert_into(version_known_issues::table)
			.values((
				version_known_issues::version_id.eq(version_id),
				version_known_issues::author.eq(author),
				version_known_issues::description.eq(description),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_for_version(
		db: &mut AsyncPgConnection,
		version_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::version_known_issues::dsl;
		dsl::version_known_issues
			.select(Self::as_select())
			.filter(dsl::version_id.eq(version_id))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn resolve(
		db: &mut AsyncPgConnection,
		issue_id: Uuid,
		resolved_by: &str,
		resolution_message: &str,
	) -> Result<Self> {
		use crate::schema::version_known_issues::dsl;
		let now = Timestamp::now();
		diesel::update(dsl::version_known_issues)
			.filter(dsl::id.eq(issue_id))
			.filter(dsl::resolved_at.is_null())
			.set((
				dsl::resolved_at.eq(jiff_diesel::NullableTimestamp::from(Some(now))),
				dsl::resolved_by.eq(resolved_by),
				dsl::resolution_message.eq(resolution_message),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Return the set of version IDs (from `ids`) that have at least one
	/// unresolved known issue. Used to compute the `ready` flag in batch.
	pub async fn versions_with_open(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashSet<Uuid>> {
		use crate::schema::version_known_issues::dsl;
		if ids.is_empty() {
			return Ok(std::collections::HashSet::new());
		}
		let rows: Vec<Uuid> = dsl::version_known_issues
			.select(dsl::version_id)
			.filter(dsl::version_id.eq_any(ids))
			.filter(dsl::resolved_at.is_null())
			.distinct()
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows.into_iter().collect())
	}

	pub async fn version_is_ready(db: &mut AsyncPgConnection, version_id: Uuid) -> Result<bool> {
		use crate::schema::version_known_issues::dsl;
		let count: i64 = dsl::version_known_issues
			.filter(dsl::version_id.eq(version_id))
			.filter(dsl::resolved_at.is_null())
			.count()
			.get_result(db)
			.await
			.map_err(AppError::from)?;
		Ok(count == 0)
	}
}
