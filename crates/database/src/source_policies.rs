//! Operator policy per reporting source. Currently the reachability mode
//! (how the source's silence bears on its servers' reachability); the
//! ingest mode follows in a later change. Absent rows mean the defaults.

use std::collections::HashMap;

use commons_errors::{AppError, Result};
use commons_types::source::ReachabilityMode;
use diesel::prelude::*;
use diesel::sql_types;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;

/// A reporting source with its reachability policy and fleet-wide
/// last-seen, for the operator source list. Reserved sources are excluded.
#[derive(Clone, Debug)]
pub struct SourceInfo {
	pub source: String,
	pub reachability: ReachabilityMode,
	pub last_seen: Option<Timestamp>,
}

/// One source's policy row.
#[derive(Clone, Debug, Queryable, Selectable)]
#[diesel(table_name = crate::schema::source_policies)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SourcePolicy {
	pub source: String,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub reachability: ReachabilityMode,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

impl SourcePolicy {
	/// Every source policy row, ordered by source.
	pub async fn list(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::source_policies::dsl;
		dsl::source_policies
			.select(Self::as_select())
			.order(dsl::source.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Every non-reserved reporting source (those with catalogued checks),
	/// with its reachability mode (defaulting to `on`) and the most recent
	/// time any of its checks was seen fleet-wide. Ordered by source.
	pub async fn list_sources(db: &mut AsyncPgConnection) -> Result<Vec<SourceInfo>> {
		#[derive(QueryableByName)]
		struct Row {
			#[diesel(sql_type = sql_types::Text)]
			source: String,
			#[diesel(sql_type = sql_types::Text)]
			reachability: String,
			#[diesel(sql_type = sql_types::Nullable<sql_types::Timestamptz>)]
			last_seen: Option<jiff_diesel::Timestamp>,
		}
		let rows: Vec<Row> = diesel::sql_query(
			"SELECT cp.source, \
			 coalesce(sp.reachability, 'on') AS reachability, \
			 max(cp.last_seen) AS last_seen \
			 FROM check_policies cp \
			 LEFT JOIN source_policies sp ON sp.source = cp.source \
			 WHERE cp.source NOT IN ('canopy', 'manual') \
			 GROUP BY cp.source, sp.reachability \
			 ORDER BY cp.source",
		)
		.load(db)
		.await?;
		Ok(rows
			.into_iter()
			.map(|r| SourceInfo {
				source: r.source,
				reachability: r.reachability.parse().unwrap_or_default(),
				last_seen: r.last_seen.map(Into::into),
			})
			.collect())
	}

	/// Each source's reachability mode. Sources without a row are absent
	/// here; callers default them to [`ReachabilityMode::On`].
	pub async fn modes(db: &mut AsyncPgConnection) -> Result<HashMap<String, ReachabilityMode>> {
		use crate::schema::source_policies::dsl;
		let rows: Vec<(String, String)> = dsl::source_policies
			.select((dsl::source, dsl::reachability))
			.load(db)
			.await?;
		Ok(rows
			.into_iter()
			.filter_map(|(source, mode)| mode.parse().ok().map(|m| (source, m)))
			.collect())
	}

	/// Set a source's reachability mode, creating its policy row if needed.
	pub async fn set_reachability(
		db: &mut AsyncPgConnection,
		source: &str,
		mode: ReachabilityMode,
	) -> Result<()> {
		use crate::schema::source_policies::dsl;
		diesel::insert_into(dsl::source_policies)
			.values((
				dsl::source.eq(source),
				dsl::reachability.eq(mode.to_string()),
			))
			.on_conflict(dsl::source)
			.do_update()
			.set((
				dsl::reachability.eq(mode.to_string()),
				dsl::updated_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
			))
			.execute(db)
			.await?;
		Ok(())
	}
}
