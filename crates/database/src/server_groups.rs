//! Server groups: a flat unit grouping several servers together for the
//! purposes of incident roll-up, shared tags, and shared operator notes.

use commons_errors::{AppError, Result};
use commons_types::server::{TagMap, rank::ServerRank};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::servers::Server;

fn higher_rank(a: ServerRank, b: ServerRank) -> ServerRank {
	let order = |r: ServerRank| match r {
		ServerRank::Production => 0u8,
		ServerRank::Clone => 1,
		ServerRank::Demo => 2,
		ServerRank::Test => 3,
		ServerRank::Dev => 4,
	};
	if order(a) <= order(b) { a } else { b }
}

#[derive(
	Debug,
	Clone,
	Serialize,
	Deserialize,
	Queryable,
	Selectable,
	Insertable,
	AsChangeset,
	utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::server_groups)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroup {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	pub name: String,
	#[serde(default)]
	pub notes: String,
	#[serde(default)]
	pub tags: TagMap,
}

#[derive(Debug, Clone, Deserialize, Insertable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_groups)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewServerGroup {
	pub name: String,
	#[serde(default)]
	pub notes: String,
	#[serde(default)]
	pub tags: TagMap,
}

#[derive(Debug, Clone, Deserialize, AsChangeset, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_groups)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PartialServerGroup {
	pub name: Option<String>,
	pub notes: Option<String>,
	pub tags: Option<TagMap>,
}

impl ServerGroup {
	pub async fn create(db: &mut AsyncPgConnection, new: NewServerGroup) -> Result<Self> {
		use crate::schema::server_groups;
		diesel::insert_into(server_groups::table)
			.values(new)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Self> {
		use crate::schema::server_groups::dsl;
		dsl::server_groups
			.select(Self::as_select())
			.filter(dsl::id.eq(group_id))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::server_groups::dsl;
		dsl::server_groups
			.select(Self::as_select())
			.order(dsl::name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_by_ids(db: &mut AsyncPgConnection, ids: &[Uuid]) -> Result<Vec<Self>> {
		use crate::schema::server_groups::dsl;
		if ids.is_empty() {
			return Ok(Vec::new());
		}
		dsl::server_groups
			.select(Self::as_select())
			.filter(dsl::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn update(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		changes: PartialServerGroup,
	) -> Result<Self> {
		use crate::schema::server_groups::dsl;
		diesel::update(dsl::server_groups.filter(dsl::id.eq(group_id)))
			.set(changes)
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Self::get_by_id(db, group_id).await
	}

	/// Refuses to delete a group that still has servers attached — operators
	/// should move servers out first. (Postgres' ON DELETE SET NULL on
	/// `servers.group_id` would happily turn the constraint into a silent
	/// data drift, so we guard at the application layer.)
	pub async fn delete(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<()> {
		use crate::schema::{server_groups, servers};

		let attached: i64 = servers::table
			.count()
			.filter(servers::group_id.eq(group_id))
			.get_result(db)
			.await?;
		if attached > 0 {
			return Err(AppError::Conflict(format!(
				"group {group_id} still has {attached} server(s); move them out first",
			)));
		}

		diesel::delete(server_groups::table.filter(server_groups::id.eq(group_id)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	pub async fn list_servers(&self, db: &mut AsyncPgConnection) -> Result<Vec<Server>> {
		use crate::schema::servers::dsl;
		dsl::servers
			.select(Server::as_select())
			.filter(dsl::group_id.eq(self.id))
			.order(dsl::name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// For each group id, the rank of its highest-ranked member. Used by the
	/// Status page to bucket groups by rank (Production beats Clone, Demo,
	/// Test, Dev). Groups without ranked members are absent from the map.
	pub async fn highest_member_ranks(
		db: &mut AsyncPgConnection,
		group_ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, commons_types::server::rank::ServerRank>> {
		use crate::schema::servers::dsl;
		use std::collections::HashMap;

		if group_ids.is_empty() {
			return Ok(HashMap::new());
		}
		let rows: Vec<(Uuid, Option<String>)> = dsl::servers
			.select((dsl::group_id.assume_not_null(), dsl::rank))
			.filter(dsl::group_id.eq_any(group_ids))
			.load(db)
			.await?;

		let mut out: HashMap<Uuid, commons_types::server::rank::ServerRank> = HashMap::new();
		for (gid, rank_str) in rows {
			let Some(rank): Option<commons_types::server::rank::ServerRank> =
				rank_str.and_then(|s| s.parse().ok())
			else {
				continue;
			};
			let cur = out.get(&gid).copied();
			let better = match cur {
				None => rank,
				Some(existing) => higher_rank(existing, rank),
			};
			out.insert(gid, better);
		}
		Ok(out)
	}

	/// Loose search by UUID or name substring, capped at 50 results, used by
	/// the admin UI's group picker.
	pub async fn search(db: &mut AsyncPgConnection, query: &str) -> Result<Vec<Self>> {
		use crate::schema::server_groups::dsl;
		let pattern = format!("%{}%", query);

		if let Ok(qid) = query.parse::<Uuid>()
			&& let Ok(group) = Self::get_by_id(db, qid).await
		{
			return Ok(vec![group]);
		}

		dsl::server_groups
			.select(Self::as_select())
			.filter(dsl::name.ilike(&pattern))
			.order(dsl::name.asc())
			.limit(50)
			.load(db)
			.await
			.map_err(AppError::from)
	}
}
