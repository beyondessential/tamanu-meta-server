//! Reporting-schema builds: which pairs of group and Tamanu version have a
//! schema, which have been tried, and which an operator has asked for again.
//!
//! spec: RPT

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
	restore::{BackupRestoreCheck, NewBackupRestoreCheck},
	versions::Version,
};
use commons_types::backup::RunOutcome;

/// A build of one pair, hanging off the restore report that carries the
/// replica's own health.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::reporting_schema_builds)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ReportingSchemaBuild {
	/// The restore report this build was reported with.
	pub check_id: i64,
	/// The group the schema was built for.
	pub group_id: Uuid,
	/// The Tamanu version the schema was built for.
	pub version_id: Uuid,
	/// The central application whose snapshot the replica was restored from.
	pub application_id: Option<Uuid>,
	/// Whether a schema came out of it.
	pub built: bool,
	/// What went wrong, where it did not.
	pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewReportingSchemaBuild {
	pub group_id: Uuid,
	pub version_id: Uuid,
	pub application_id: Option<Uuid>,
	pub built: bool,
	pub error: Option<String>,
}

impl ReportingSchemaBuild {
	/// Record a build: the replica's restore report first, then the build that
	/// rode on it.
	// spec: RPT#what-a-build-reports
	pub async fn record(
		db: &mut AsyncPgConnection,
		report: NewBackupRestoreCheck,
		build: NewReportingSchemaBuild,
	) -> Result<i64> {
		let restore_failed = report.outcome != RunOutcome::Success;

		let check_id = BackupRestoreCheck::record_report(db, report).await?;

		// A replica that failed to restore says nothing about whether the pair
		// can be built: the build never ran. Restore-health already raises on
		// that, and recording no build leaves the pair unsettled so it is
		// dispatched again, which is what an unhealthy restore should do.
		if restore_failed {
			return Ok(check_id);
		}

		diesel::insert_into(crate::schema::reporting_schema_builds::table)
			.values((
				crate::schema::reporting_schema_builds::check_id.eq(check_id),
				crate::schema::reporting_schema_builds::group_id.eq(build.group_id),
				crate::schema::reporting_schema_builds::version_id.eq(build.version_id),
				crate::schema::reporting_schema_builds::application_id.eq(build.application_id),
				crate::schema::reporting_schema_builds::built.eq(build.built),
				crate::schema::reporting_schema_builds::error.eq(build.error),
			))
			.execute(db)
			.await?;

		// An operator's ask is answered once the build it asked for lands,
		// whichever way it went.
		ReportingSchemaRequest::clear(db, build.group_id, build.version_id).await?;

		Ok(check_id)
	}

	/// The most recent build of a pair, if it has been tried.
	pub async fn latest_for_pair(
		db: &mut AsyncPgConnection,
		group: Uuid,
		version: Uuid,
	) -> Result<Option<Self>> {
		use crate::schema::{backup_restore_checks, reporting_schema_builds};

		reporting_schema_builds::table
			.inner_join(
				backup_restore_checks::table
					.on(backup_restore_checks::id.eq(reporting_schema_builds::check_id)),
			)
			.filter(reporting_schema_builds::group_id.eq(group))
			.filter(reporting_schema_builds::version_id.eq(version))
			.order_by(backup_restore_checks::reported_at.desc())
			.select(Self::as_select())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Whether a pair is settled: it has been built or has failed, and either
	/// way is not dispatched again until the version's artifacts change or an
	/// operator asks.
	// spec: RPT#pairs
	pub async fn is_settled(
		db: &mut AsyncPgConnection,
		group: Uuid,
		version: Uuid,
	) -> Result<bool> {
		if ReportingSchemaRequest::pending(db, group, version).await? {
			return Ok(false);
		}

		Ok(Self::latest_for_pair(db, group, version).await?.is_some())
	}
}

/// An operator asking for a pair's build.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::reporting_schema_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ReportingSchemaRequest {
	pub group_id: Uuid,
	pub version_id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub requested_at: Timestamp,
	pub requested_by: Option<String>,
}

impl ReportingSchemaRequest {
	/// Enqueue, or refresh, an ask for a pair.
	// spec: RPT#pairs
	pub async fn enqueue(
		db: &mut AsyncPgConnection,
		group: Uuid,
		version: Uuid,
		requested_by: Option<&str>,
	) -> Result<()> {
		use crate::schema::reporting_schema_requests::dsl;

		diesel::insert_into(dsl::reporting_schema_requests)
			.values((
				dsl::group_id.eq(group),
				dsl::version_id.eq(version),
				dsl::requested_by.eq(requested_by),
			))
			.on_conflict((dsl::group_id, dsl::version_id))
			.do_update()
			.set((
				dsl::requested_at.eq(diesel::dsl::now),
				dsl::requested_by.eq(requested_by),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;

		Ok(())
	}

	pub async fn pending(db: &mut AsyncPgConnection, group: Uuid, version: Uuid) -> Result<bool> {
		use crate::schema::reporting_schema_requests::dsl;

		Ok(dsl::reporting_schema_requests
			.filter(dsl::group_id.eq(group))
			.filter(dsl::version_id.eq(version))
			.select(dsl::group_id)
			.first::<Uuid>(db)
			.await
			.optional()
			.map_err(AppError::from)?
			.is_some())
	}

	async fn clear(db: &mut AsyncPgConnection, group: Uuid, version: Uuid) -> Result<()> {
		use crate::schema::reporting_schema_requests::dsl;

		diesel::delete(
			dsl::reporting_schema_requests
				.filter(dsl::group_id.eq(group))
				.filter(dsl::version_id.eq(version)),
		)
		.execute(db)
		.await
		.map_err(AppError::from)?;

		Ok(())
	}
}

/// Where a pair stands, for the operator view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PairState {
	/// No build has been recorded, so the pair is on the worklist.
	Awaiting,
	/// A build produced a schema.
	Built,
	/// A build ran and produced none.
	Failed,
}

/// One pair of group and Tamanu version, and where it stands.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Pair {
	pub group_id: Uuid,
	pub version_id: Uuid,
	pub version: String,
	pub state: PairState,
	/// What went wrong, where a build failed.
	pub error: Option<String>,
	/// Whether an operator has asked for this pair to be built again.
	pub requested: bool,
}

/// The pairs of a group: every published version its Tamanu applications report
/// running, plus the version its open plan moves it to.
// spec: RPT#pairs
pub async fn pairs_for_group(db: &mut AsyncPgConnection, group: Uuid) -> Result<Vec<Pair>> {
	let mut versions = versions_for_group(db, group).await?;
	versions.sort_by_key(|v| (v.major, v.minor, v.patch));
	versions.dedup_by_key(|v| v.id);

	let mut pairs = Vec::with_capacity(versions.len());
	for version in versions {
		let latest = ReportingSchemaBuild::latest_for_pair(db, group, version.id).await?;
		let requested = ReportingSchemaRequest::pending(db, group, version.id).await?;

		let (state, error) = match &latest {
			None => (PairState::Awaiting, None),
			Some(build) if build.built => (PairState::Built, None),
			Some(build) => (PairState::Failed, build.error.clone()),
		};

		pairs.push(Pair {
			group_id: group,
			version_id: version.id,
			version: version.as_semver().to_string(),
			state,
			error,
			requested,
		});
	}

	Ok(pairs)
}

/// Every published version a group's Tamanu applications report running, plus
/// the version its open plan moves it to.
///
/// A reported version Canopy holds no release row for is not a pair: a build
/// needs that version's migrations, which reach a builder as its published
/// artifacts.
// spec: RPT#pairs
pub async fn versions_for_group(db: &mut AsyncPgConnection, group: Uuid) -> Result<Vec<Version>> {
	use commons_types::version::VersionStatus;

	let applications = crate::applications::Application::list_live_in_group(db, group).await?;
	let tamanu: Vec<Uuid> = applications
		.iter()
		.filter(|a| a.r#type.software() == "tamanu")
		.map(|a| a.id)
		.collect();

	let mut versions = Vec::new();

	let reported = crate::reported_detail::ReportedDetail::last_versions(db, &tamanu).await?;
	for shown in reported.into_values() {
		// A version Canopy holds no release row for is not a pair: a build needs
		// that version's migrations, which reach a builder as published artifacts.
		if let Ok(version) = Version::get_by_version(db, shown).await
			&& version.status == VersionStatus::Published
		{
			versions.push(version);
		}
	}

	if let Some(target) = crate::upgrade_plans::planned_target(db, group).await? {
		versions.push(target);
	}

	Ok(versions)
}
