//! Operator policy per reporting source. Currently the reachability mode
//! (how the source's silence bears on its servers' reachability); the
//! ingest mode follows in a later change. Absent rows mean the defaults.

use std::collections::HashMap;

use commons_errors::{AppError, Result};
use commons_types::source::ReachabilityMode;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;

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
