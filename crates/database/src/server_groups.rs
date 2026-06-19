//! Server groups: a flat unit grouping several servers together for the
//! purposes of incident roll-up, shared tags, and shared operator notes.

use commons_errors::{AppError, Result};
use commons_types::server::{TagMap, kind::ServerKind, rank::ServerRank};
use commons_types::version::VersionStr;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::pg_duration::PgDuration;
use crate::servers::Server;
use crate::statuses::Status;

/// Ordering key for a server's rank — lower is higher priority. Used to pick a
/// group's canonical member (and to bucket groups on the status page). `None`
/// (unranked) sorts last.
pub fn rank_priority(rank: Option<ServerRank>) -> u8 {
	match rank {
		Some(ServerRank::Production) => 0,
		Some(ServerRank::Clone) => 1,
		Some(ServerRank::Demo) => 2,
		Some(ServerRank::Test) => 3,
		Some(ServerRank::Dev) => 4,
		None => 5,
	}
}

/// Ordering key for a server's kind — lower is higher priority. Central servers
/// are the headline of a group; facility ties below them, canopy-kind last.
pub fn kind_priority(kind: ServerKind) -> u8 {
	match kind {
		ServerKind::Central => 0,
		ServerKind::Facility => 1,
		ServerKind::Canopy => 2,
	}
}

fn higher_rank(a: ServerRank, b: ServerRank) -> ServerRank {
	if rank_priority(Some(a)) <= rank_priority(Some(b)) {
		a
	} else {
		b
	}
}

#[derive(
	Debug,
	Clone,
	Serialize,
	Deserialize,
	Queryable,
	Selectable,
	Insertable,
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
	/// How long an `incident_open` Slack notification waits in the outbox
	/// before the drainer is allowed to ship it. A resolve that arrives
	/// inside this window cancels the open outright (and skips its own
	/// notification), so groups with chronic flap can crank this up to
	/// keep Slack quiet without losing the underlying incident record.
	#[schema(value_type = i64, format = "int64")]
	pub slack_open_delay: PgDuration,
	/// The group's canonical member (highest rank, then highest kind) whose
	/// version is cached in `effective_version`. Maintained by
	/// [`ServerGroup::recompute_version`] on membership/rank/kind/delete
	/// changes. `None` when the group has no members.
	pub version_server_id: Option<Uuid>,
	/// The canonical member's last reported version. Maintained by the
	/// `statuses` AFTER INSERT trigger (canonical member reports a new version)
	/// and by [`ServerGroup::recompute_version`] (membership changes).
	pub effective_version: Option<VersionStr>,
	/// When set, the group is archived (soft-deleted): hidden from live listings
	/// but kept (with its archived members) and restorable.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub deleted_at: Option<Timestamp>,
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
	#[serde(default)]
	#[schema(value_type = Option<i64>, format = "int64")]
	pub slack_open_delay: Option<PgDuration>,
}

#[derive(Debug, Clone, Deserialize, AsChangeset, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_groups)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PartialServerGroup {
	pub name: Option<String>,
	pub notes: Option<String>,
	pub tags: Option<TagMap>,
	#[schema(value_type = Option<i64>, format = "int64")]
	pub slack_open_delay: Option<PgDuration>,
}

impl ServerGroup {
	pub async fn create(db: &mut AsyncPgConnection, new: NewServerGroup) -> Result<Self> {
		use crate::schema::server_groups;
		crate::tags::reject_reserved_keys(&new.tags)?;
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
			.filter(dsl::deleted_at.is_null())
			.order(dsl::name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Archived (soft-deleted) groups, newest-first. Powers the Archived view.
	pub async fn list_archived(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::server_groups::dsl;
		dsl::server_groups
			.select(Self::as_select())
			.filter(dsl::deleted_at.is_not_null())
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
			.filter(dsl::deleted_at.is_null())
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
		if let Some(tags) = &changes.tags {
			crate::tags::reject_reserved_keys(tags)?;
		}
		diesel::update(dsl::server_groups.filter(dsl::id.eq(group_id)))
			.set(changes)
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Self::get_by_id(db, group_id).await
	}

	/// Archive (soft-delete) a group, hiding it from live listings while keeping
	/// it (and its members) and allowing restore.
	///
	/// An empty group archives outright. A group with live members archives only
	/// if *every* live member is **gone** (no status in the last 7 days — the
	/// same notion the UI shows): in that case the archive **cascades**, also
	/// archiving those members. If any live member reported recently, it refuses
	/// (409) — you don't bulk-archive a group with active servers.
	pub async fn soft_delete(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<()> {
		use crate::schema::{server_groups::dsl, servers};
		use diesel_async::AsyncConnection;

		db.transaction::<_, AppError, _>(async |conn| {
			let member_ids: Vec<Uuid> = servers::table
				.select(servers::id)
				.filter(servers::group_id.eq(group_id))
				.filter(servers::deleted_at.is_null())
				.load(conn)
				.await
				.map_err(AppError::from)?;

			if !member_ids.is_empty() {
				// A member is "gone" iff it's absent from `latest_for_servers`
				// (no status in the last 7 days). Allow the cascade only when
				// every live member is gone; any recent reporter blocks it.
				let recent = Status::latest_for_servers(conn, &member_ids).await?;
				if !recent.is_empty() {
					return Err(AppError::Conflict(format!(
						"group {group_id} has {} server(s) that reported within the last \
						 week; only a group whose servers are all gone can be archived",
						recent.len(),
					)));
				}
				for id in &member_ids {
					Server::soft_delete(conn, *id).await?;
				}
			}

			diesel::update(dsl::server_groups.filter(dsl::id.eq(group_id)))
				.set(
					dsl::deleted_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
				)
				.execute(conn)
				.await
				.map_err(AppError::from)?;
			Ok(())
		})
		.await
	}

	/// Un-archive a group, cascading to restore its archived members (the
	/// inverse of [`ServerGroup::soft_delete`]'s cascade).
	pub async fn restore(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<()> {
		use crate::schema::{server_groups::dsl, servers};
		use diesel_async::AsyncConnection;

		db.transaction::<_, AppError, _>(async |conn| {
			let archived_members: Vec<Uuid> = servers::table
				.select(servers::id)
				.filter(servers::group_id.eq(group_id))
				.filter(servers::deleted_at.is_not_null())
				.load(conn)
				.await
				.map_err(AppError::from)?;
			for id in &archived_members {
				Server::restore(conn, *id).await?;
			}

			diesel::update(dsl::server_groups.filter(dsl::id.eq(group_id)))
				.set(dsl::deleted_at.eq(None::<jiff_diesel::Timestamp>))
				.execute(conn)
				.await
				.map_err(AppError::from)?;
			Ok(())
		})
		.await
	}

	pub async fn list_servers(&self, db: &mut AsyncPgConnection) -> Result<Vec<Server>> {
		use crate::schema::servers::dsl;
		dsl::servers
			.select(Server::as_select())
			.filter(dsl::group_id.eq(self.id))
			.filter(dsl::deleted_at.is_null())
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
			.filter(dsl::deleted_at.is_null())
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

	/// Count of live (non-archived) servers in each group, keyed by group id.
	/// Groups with no live members are absent (callers default to 0).
	pub async fn live_server_counts(
		db: &mut AsyncPgConnection,
	) -> Result<std::collections::HashMap<Uuid, i64>> {
		use crate::schema::servers::dsl;
		use std::collections::HashMap;

		let group_ids: Vec<Uuid> = dsl::servers
			.select(dsl::group_id.assume_not_null())
			.filter(dsl::group_id.is_not_null())
			.filter(dsl::deleted_at.is_null())
			.load(db)
			.await?;

		let mut counts: HashMap<Uuid, i64> = HashMap::new();
		for gid in group_ids {
			*counts.entry(gid).or_insert(0) += 1;
		}
		Ok(counts)
	}

	/// Recompute the cached canonical member and its version for `group_id`.
	///
	/// Loads every member of the group, picks the canonical one (lowest
	/// `(rank_priority, kind_priority)`, tie-broken by `id`), and caches that
	/// member's last version-bearing status. No members → both cache columns
	/// are cleared. Runs only on infrequent membership/rank/kind/delete
	/// changes, so the unbounded `last_with_version_for_server` query is fine
	/// here. The `statuses` trigger handles the hot path (canonical member
	/// reporting a new version) without touching this.
	pub async fn recompute_version(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<()> {
		use crate::schema::server_groups::dsl;

		let members: Vec<Server> = {
			use crate::schema::servers::dsl as servers_dsl;
			servers_dsl::servers
				.select(Server::as_select())
				.filter(servers_dsl::group_id.eq(group_id))
				.filter(servers_dsl::deleted_at.is_null())
				.load(db)
				.await?
		};

		let canonical = members.into_iter().min_by(|a, b| {
			(rank_priority(a.rank), kind_priority(a.kind), a.id).cmp(&(
				rank_priority(b.rank),
				kind_priority(b.kind),
				b.id,
			))
		});

		let (version_server_id, effective_version) = match canonical {
			None => (None, None),
			Some(server) => {
				let version = Status::last_with_version_for_server(db, server.id)
					.await?
					.and_then(|s| s.version);
				(Some(server.id), version)
			}
		};

		diesel::update(dsl::server_groups.filter(dsl::id.eq(group_id)))
			.set((
				dsl::version_server_id.eq(version_server_id),
				dsl::effective_version.eq(effective_version),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Loose search by UUID or name substring, capped at 50 results, used by
	/// the admin UI's group picker.
	pub async fn search(db: &mut AsyncPgConnection, query: &str) -> Result<Vec<Self>> {
		use crate::schema::server_groups::dsl;
		let pattern = format!("%{}%", query);

		if let Ok(qid) = query.parse::<Uuid>()
			&& let Ok(group) = Self::get_by_id(db, qid).await
			&& group.deleted_at.is_none()
		{
			return Ok(vec![group]);
		}

		dsl::server_groups
			.select(Self::as_select())
			.filter(dsl::name.ilike(&pattern))
			.filter(dsl::deleted_at.is_null())
			.order(dsl::name.asc())
			.limit(50)
			.load(db)
			.await
			.map_err(AppError::from)
	}
}
