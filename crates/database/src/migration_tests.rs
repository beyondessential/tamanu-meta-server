//! Which versions Canopy asks to be migration-tested against which servers,
//! and what came back.
//!
//! Candidacy here is the version axis only. Whether a server has a snapshot to
//! restore and migrate is settled when a consumer's worklist is built.

use std::collections::BTreeMap;

use commons_errors::Result;
use commons_types::{
	backup::RunOutcome,
	server::product::Product,
	version::{VersionStatus, VersionStr},
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
	pg_duration::PgDuration, reported_detail::ReportedDetail, restore::BackupRestoreCheck,
	restore::NewBackupRestoreCheck, servers::Server, versions::Version,
};

/// A version a server could upgrade to, so one to test against that server's
/// data before it gets there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
	pub server_id: Uuid,
	pub version_id: Uuid,
}

/// The published versions `reported` could upgrade to, one per minor series.
///
/// Stays within the reported major, matching the update path Canopy serves a
/// server. Only the newest patch of each minor is returned: an upgrade applies
/// that patch, so an earlier one is never what the server ends up running, and
/// testing it would answer a question nobody asks.
// spec: RST#candidate-versions
pub fn upgrade_path(reported: &VersionStr, versions: &[Version]) -> Vec<Uuid> {
	let current = &reported.0;
	let mut newest_per_minor: BTreeMap<i32, &Version> = BTreeMap::new();

	for version in versions {
		if version.status != VersionStatus::Published || version.major != current.major as i32 {
			continue;
		}

		let is_newer = version.minor > current.minor as i32
			|| (version.minor == current.minor as i32 && version.patch > current.patch as i32);
		if !is_newer {
			continue;
		}

		newest_per_minor
			.entry(version.minor)
			.and_modify(|held| {
				if version.patch > held.patch {
					*held = version;
				}
			})
			.or_insert(version);
	}

	newest_per_minor
		.into_values()
		.map(|version| version.id)
		.collect()
}

/// Every candidate across the fleet.
///
/// Tamanu servers only: the migrations under test are Tamanu's, so no other
/// product's server has an upgrade path through them.
// spec: RST#candidate-versions
pub async fn candidates(db: &mut AsyncPgConnection) -> Result<Vec<Candidate>> {
	let versions = Version::get_all(db).await?;
	let mut candidates = Vec::new();

	for server in Server::get_all(db, 0, None).await? {
		if server.product != Product::Tamanu {
			continue;
		}

		let Some(reported) = ReportedDetail::last_version(db, server.id).await? else {
			continue;
		};

		candidates.extend(
			upgrade_path(&reported, &versions)
				.into_iter()
				.map(|version_id| Candidate {
					server_id: server.id,
					version_id,
				}),
		);
	}

	Ok(candidates)
}

/// How long one migration took, in the order it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::migration_timings)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MigrationTiming {
	pub ordinal: i32,
	pub name: String,
	pub elapsed: PgDuration,
}

/// A recorded migration test, alongside the report carrying its common fields.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::migration_tests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MigrationTest {
	pub check_id: i64,
	pub target_version_id: Uuid,
	pub total_elapsed: PgDuration,
	pub failed_migration: Option<String>,
	pub data_bytes_before: i64,
	pub data_bytes_after: i64,
}

/// What a consumer reports for one migration test, beyond the report's own
/// fields.
#[derive(Debug, Clone)]
pub struct NewMigrationTest {
	pub target_version_id: Uuid,
	pub total_elapsed: PgDuration,
	pub failed_migration: Option<String>,
	pub data_bytes_before: i64,
	pub data_bytes_after: i64,
	/// One entry per migration that ran, in the order they ran.
	pub timings: Vec<(String, PgDuration)>,
}

/// Where a (server, version) pair stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
	NotTested,
	Passed,
	Failed,
}

impl MigrationTest {
	/// Record a migration test: the report that carries the common fields,
	/// then the result and its per-migration timings.
	// spec: RST#what-a-migration-test-reports
	pub async fn record(
		db: &mut AsyncPgConnection,
		report: NewBackupRestoreCheck,
		test: NewMigrationTest,
	) -> Result<i64> {
		let check_id = BackupRestoreCheck::record_report(db, report).await?;

		diesel::insert_into(crate::schema::migration_tests::table)
			.values((
				crate::schema::migration_tests::check_id.eq(check_id),
				crate::schema::migration_tests::target_version_id.eq(test.target_version_id),
				crate::schema::migration_tests::total_elapsed.eq(test.total_elapsed),
				crate::schema::migration_tests::failed_migration.eq(test.failed_migration),
				crate::schema::migration_tests::data_bytes_before.eq(test.data_bytes_before),
				crate::schema::migration_tests::data_bytes_after.eq(test.data_bytes_after),
			))
			.execute(db)
			.await?;

		let timings: Vec<_> = test
			.timings
			.into_iter()
			.enumerate()
			.map(|(index, (name, elapsed))| {
				(
					crate::schema::migration_timings::check_id.eq(check_id),
					crate::schema::migration_timings::ordinal.eq(index as i32),
					crate::schema::migration_timings::name.eq(name),
					crate::schema::migration_timings::elapsed.eq(elapsed),
				)
			})
			.collect();
		if !timings.is_empty() {
			diesel::insert_into(crate::schema::migration_timings::table)
				.values(timings)
				.execute(db)
				.await?;
		}

		Ok(check_id)
	}

	/// The per-migration timings of one test, in the order they ran.
	pub async fn timings(
		db: &mut AsyncPgConnection,
		check_id: i64,
	) -> Result<Vec<MigrationTiming>> {
		use crate::schema::migration_timings::dsl;

		dsl::migration_timings
			.select(MigrationTiming::as_select())
			.filter(dsl::check_id.eq(check_id))
			.order(dsl::ordinal.asc())
			.load(db)
			.await
			.map_err(Into::into)
	}
}

/// Where `server` stands against `version`, from its most recent test.
///
/// A pass means every migration applied. Anything else is a failure, including
/// a report whose restore never got as far as migrating.
// spec: RST#verdicts
pub async fn verdict(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	target_version_id: Uuid,
) -> Result<Verdict> {
	use crate::schema::{backup_restore_checks, migration_tests};

	let latest: Option<(RunOutcome, Option<String>)> = migration_tests::table
		.inner_join(backup_restore_checks::table)
		.select((
			backup_restore_checks::outcome,
			migration_tests::failed_migration,
		))
		.filter(migration_tests::target_version_id.eq(target_version_id))
		.filter(backup_restore_checks::server_id.eq(server_id))
		.order(backup_restore_checks::reported_at.desc())
		.first(db)
		.await
		.optional()?;

	Ok(match latest {
		None => Verdict::NotTested,
		Some((RunOutcome::Success, None)) => Verdict::Passed,
		Some(_) => Verdict::Failed,
	})
}
