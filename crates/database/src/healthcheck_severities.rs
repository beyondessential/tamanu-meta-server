//! Operator-owned catalog of healthcheck names → the severity to file
//! their failures at. See `docs/plans/healthcheck-severity-catalog.md`.
//!
//! Ingestion (in the public-server status handler) calls
//! [`HealthcheckSeverity::upsert_default`] for every check name seen on
//! a push, then [`HealthcheckSeverity::severity_for`] when filing a
//! failing per-check issue. Operators read and edit the catalog via the
//! private-server `/api/healthchecks` endpoints.

use commons_errors::{AppError, Result};
use commons_types::issue::Severity;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::healthcheck_severities)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct HealthcheckSeverity {
	pub check_name: String,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub severity: Severity,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub first_seen: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub reviewed_at: Option<Timestamp>,
	pub reviewed_by: Option<String>,
	pub notes: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

impl HealthcheckSeverity {
	/// Insert a row for `check_name` with default values (severity =
	/// warning, reviewed_at = NULL) if and only if no row exists yet.
	/// Idempotent: safe to call on every status push for every check
	/// seen, including healthy ones. Concurrent pushes are serialised
	/// by Postgres via `ON CONFLICT DO NOTHING`.
	pub async fn upsert_default(db: &mut AsyncPgConnection, check_name: &str) -> Result<()> {
		use crate::schema::healthcheck_severities::dsl;
		diesel::insert_into(dsl::healthcheck_severities)
			.values(dsl::check_name.eq(check_name))
			.on_conflict(dsl::check_name)
			.do_nothing()
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Look up the severity assigned to `check_name`. Falls back to
	/// `Severity::Warning` if no row exists yet — in practice the
	/// status handler upserts before reading, so this branch only
	/// covers the genuine race / programmer-error case.
	pub async fn severity_for(db: &mut AsyncPgConnection, check_name: &str) -> Result<Severity> {
		use crate::schema::healthcheck_severities::dsl;
		let row: Option<String> = dsl::healthcheck_severities
			.select(dsl::severity)
			.filter(dsl::check_name.eq(check_name))
			.first(db)
			.await
			.optional()?;
		Ok(row
			.map(|s| s.parse().unwrap_or(Severity::Warning))
			.unwrap_or(Severity::Warning))
	}

	pub async fn list(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::healthcheck_severities::dsl;
		dsl::healthcheck_severities
			.select(Self::as_select())
			.order(dsl::check_name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Update the severity (and optionally notes) for a check, stamping
	/// `reviewed_at = NOW()` and `reviewed_by = by`. Even a no-op save
	/// (same severity) marks the row reviewed — operators can ack
	/// a check without changing it.
	pub async fn update(
		db: &mut AsyncPgConnection,
		check_name: &str,
		severity: Severity,
		notes: Option<&str>,
		by: &str,
	) -> Result<Self> {
		use crate::schema::healthcheck_severities::dsl;
		let now = Timestamp::now();
		diesel::update(dsl::healthcheck_severities.filter(dsl::check_name.eq(check_name)))
			.set((
				dsl::severity.eq(severity),
				dsl::notes.eq(notes),
				dsl::reviewed_at.eq(jiff_diesel::Timestamp::from(now)),
				dsl::reviewed_by.eq(by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}
}
