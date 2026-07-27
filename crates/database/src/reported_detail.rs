//! Each source's current server-wide detail, as a table rather than a
//! search through status history.
//!
//! The same facts are in `statuses.extra`, but that table is partitioned by
//! week and a predicate on `server_id` alone can't be pruned, so resolving
//! one server's figures means a bounded scan over recent partitions —
//! affordable once per page, not once per server on a fleet-wide view. This
//! is the current-state projection: ingest keeps it fresh, and every read
//! that only wants "what is this server running now" stops here.
//!
//! Status history stays the record of what was reported *when*; this is the
//! record of what stands.

use commons_errors::{AppError, Result};
use commons_types::{server::rank::ServerRank, version::VersionStr};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::statuses::MergedDetail;

/// How recently a server must have reported to count as still running what it
/// last reported.
///
/// Most reads here deliberately have no such bound — a figure is what the
/// server runs, and that doesn't stop being true because the server went
/// quiet. But "what is the fleet *actively* running" is a different question:
/// a decommissioned server that was never archived would otherwise keep its
/// release branch in the count forever.
const ACTIVE_LOOKBACK_SQL: &str = "NOW() - INTERVAL '7 days'";

/// One source's latest server-wide detail for one server.
// spec: FIG#sourcing
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::server_reported_detail)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ReportedDetail {
	/// The server this detail describes.
	pub server_id: Uuid,
	/// The source that reported it.
	pub source: String,
	/// That source's whole server-wide detail as last pushed.
	pub extra: serde_json::Value,
	/// The application version that push reported, if it reported one.
	pub version: Option<VersionStr>,
	/// When the push carrying this detail landed.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub reported_at: Timestamp,
}

impl ReportedDetail {
	/// Record `source`'s current detail for `server`, replacing what it
	/// reported before. A push is the source's whole truth, so this is a
	/// replace and not a merge; other sources' rows are untouched.
	///
	/// The version is the exception: a push that carries none keeps the last
	/// one this source reported. An agent omits the version when it can't
	/// read it — the application is down, or mid-upgrade — which says nothing
	/// about what the server is installed to run, and blanking on it would
	/// make a group's headline version flicker off exactly when an operator
	/// is looking at it.
	pub async fn record(
		db: &mut AsyncPgConnection,
		server: Uuid,
		source: &str,
		extra: &serde_json::Value,
		version: Option<&VersionStr>,
	) -> Result<()> {
		use crate::schema::server_reported_detail::dsl;

		diesel::insert_into(dsl::server_reported_detail)
			.values((
				dsl::server_id.eq(server),
				dsl::source.eq(source),
				dsl::extra.eq(extra),
				dsl::version.eq(version),
				dsl::reported_at.eq(diesel::dsl::now),
			))
			.on_conflict((dsl::server_id, dsl::source))
			.do_update()
			.set((
				dsl::extra.eq(extra),
				// COALESCE over the excluded row: a version-less push keeps
				// the version this source last reported.
				dsl::version.eq(diesel::dsl::sql::<
					diesel::sql_types::Nullable<diesel::sql_types::Text>,
				>(
					"COALESCE(EXCLUDED.version, server_reported_detail.version)",
				)),
				dsl::reported_at.eq(diesel::dsl::now),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Every source's current detail for one server.
	pub async fn for_server(db: &mut AsyncPgConnection, server: Uuid) -> Result<Vec<Self>> {
		use crate::schema::server_reported_detail::dsl;

		dsl::server_reported_detail
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Every source's current detail for every server that has any. Small
	/// enough to read whole — one row per (server, source) across the fleet.
	pub async fn all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::server_reported_detail::dsl;

		dsl::server_reported_detail
			.select(Self::as_select())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The last application version `server` reported, from the most recent
	/// source to report one.
	///
	/// Unbounded by design: this answers "what was it running", which stays
	/// true however long the server has been down — a group's headline
	/// version shouldn't blank out because its canonical member went quiet.
	/// Reading the current-detail table is what makes that affordable; the
	/// same question against status history needed a lookback cap.
	// spec: FIG#sourcing
	pub async fn last_version(
		db: &mut AsyncPgConnection,
		server: Uuid,
	) -> Result<Option<VersionStr>> {
		use crate::schema::server_reported_detail::dsl;

		let version: Option<Option<VersionStr>> = dsl::server_reported_detail
			.select(dsl::version)
			.filter(dsl::server_id.eq(server))
			.filter(dsl::version.is_not_null())
			.order(dsl::reported_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?;

		Ok(version.flatten())
	}

	/// The application version each still-reporting production server runs,
	/// one per server.
	///
	/// A server's version is the one the most recent source to report a
	/// version gave: a source that reports none doesn't drop the server from
	/// the count just by having pushed last. Bounded by
	/// [`ACTIVE_LOOKBACK_SQL`], so this answers what is *running*, not what
	/// was last seen at any point in the past.
	// spec: FIG#active-versions
	pub async fn production_versions(db: &mut AsyncPgConnection) -> Result<Vec<VersionStr>> {
		use crate::schema::{server_reported_detail as detail, servers};

		let rows: Vec<Option<VersionStr>> = detail::table
			.inner_join(servers::table.on(servers::id.eq(detail::server_id)))
			.filter(servers::rank.eq(ServerRank::Production))
			.filter(servers::deleted_at.is_null())
			.filter(detail::version.is_not_null())
			.filter(detail::reported_at.ge(diesel::dsl::sql(ACTIVE_LOOKBACK_SQL)))
			.distinct_on(detail::server_id)
			.order((detail::server_id, detail::reported_at.desc()))
			.select(detail::version)
			.load(db)
			.await
			.map_err(AppError::from)?;

		Ok(rows.into_iter().flatten().collect())
	}

	/// Resolve one server's figures from its sources' current reports.
	// spec: FIG#sourcing
	pub fn merge(reports: &[Self]) -> MergedDetail {
		MergedDetail::from_reports(reports.iter().map(|r| (r.reported_at, &r.extra)))
	}

	/// The same resolution for a whole fleet's worth of rows, keyed by
	/// server. Rows for servers the caller didn't ask about are ignored.
	pub fn merge_by_server(
		reports: Vec<Self>,
	) -> std::collections::HashMap<Uuid, (MergedDetail, Option<VersionStr>)> {
		let mut by_server: std::collections::HashMap<Uuid, Vec<Self>> =
			std::collections::HashMap::new();
		for report in reports {
			by_server.entry(report.server_id).or_default().push(report);
		}
		by_server
			.into_iter()
			.map(|(server, mut rows)| {
				// The newest report that carried a version wins it, on the
				// same rule as any other figure: a source that reports no
				// version doesn't erase one another source reported.
				rows.sort_by_key(|r| r.reported_at);
				let version = rows.iter().rev().find_map(|r| r.version.clone());
				(server, (Self::merge(&rows), version))
			})
			.collect()
	}
}
