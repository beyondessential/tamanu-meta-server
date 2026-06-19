use commons_errors::{AppError, Result};
use commons_types::{
	geo::GeoPoint,
	server::{RESERVED_TAG_PREFIX, TagMap, kind::ServerKind, rank::ServerRank},
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::pg_duration::PgDuration;
use super::url_field::UrlField;

const TEN_MINUTES: PgDuration = PgDuration(SignedDuration::from_secs(600));

/// Recompute each distinct, present group id, deduping repeats and skipping
/// `None`. Used by the server write paths that can change a group's canonical
/// member (membership/rank/kind/delete).
async fn recompute_groups(
	db: &mut AsyncPgConnection,
	groups: impl IntoIterator<Item = Option<Uuid>>,
) -> Result<()> {
	let mut seen: Vec<Uuid> = Vec::new();
	for gid in groups.into_iter().flatten() {
		if !seen.contains(&gid) {
			seen.push(gid);
			crate::server_groups::ServerGroup::recompute_version(db, gid).await?;
		}
	}
	Ok(())
}

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Server {
	pub id: Uuid,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,

	/// The server's URL. Optional: a server may be identified solely by its
	/// bound device (e.g. a Tailscale node). Not unique. For a display URL that
	/// falls back to the tailnet hostname, see the API's `display_host`.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(treat_none_as_default_value = false)]
	pub host: Option<UrlField>,

	#[diesel(deserialize_as = String, serialize_as = String)]
	pub kind: ServerKind,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub rank: Option<ServerRank>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub device_id: Option<Uuid>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub group_id: Option<Uuid>,
	/// If `Some`, the server appears in the public `/servers` list under this
	/// name (used by the Tamanu Mobile app). `None` means not listed. Decoupled
	/// from `name` because server `name`s are scoped within a group and may not
	/// be globally meaningful.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub public_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cloud: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub geolocation: Option<GeoPoint>,
	/// Whether canopy is actively watching this server. When `false`, the
	/// reachability sweep skips it entirely and any of its issues are
	/// ignored by the incident workflow — operators flip this off for
	/// test environments and ad-hoc demos. The accompanying
	/// `alert_when_down_for` is preserved while muted so flipping back on
	/// doesn't lose the chosen threshold.
	pub is_monitored: bool,
	/// Opt-in to the retired legacy `/status` format (a push that carries no
	/// `health` array). Off by default: the new format is required and a
	/// legacy push is rejected with 400. When `true`, a legacy push is
	/// accepted but only refreshes reachability — it carries the server's
	/// last known healthchecks forward instead of clearing them, so a server
	/// straddling old and new reporters doesn't flap its health issues. Flip
	/// off (the default) once the server's reporter speaks the new format.
	pub allow_legacy_status: bool,
	/// Per-server downtime threshold: how long a server's status row may
	/// go un-updated before the canopy reachability sweep files an issue.
	/// Bump it up for flappy servers, drop it for critical ones that
	/// should page promptly. Only consulted when `is_monitored` is `true`.
	///
	/// Constrained to strictly positive by the database. Default 10
	/// minutes for newly-inserted rows. On the JSON wire this is
	/// represented as a count of whole seconds (`i64`).
	#[schema(value_type = i64)]
	pub alert_when_down_for: PgDuration,
	#[serde(default)]
	pub notes: String,
	#[serde(default)]
	pub tags: TagMap,
	/// When set, the server is archived (soft-deleted): hidden from live
	/// listings and monitoring, its device released, but its history retained.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub deleted_at: Option<Timestamp>,
	/// Set when a device successfully completes enrollment for this server.
	/// While `None`, the server is awaiting its first check-in and the UI
	/// shows setup instructions.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub registered_at: Option<Timestamp>,
}

impl Server {
	pub async fn get_all(
		db: &mut AsyncPgConnection,
		offset: u64,
		limit: Option<u64>,
	) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		let q = servers
			.select(Self::as_select())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.order_by((
				name.is_not_null(),
				kind.asc(),
				name.asc(),
				created_at.desc(),
			))
			.offset(offset.try_into().unwrap_or(i64::MAX));

		if let Some(limit) = limit {
			q.limit(limit.try_into().unwrap_or(i64::MAX)).load(db).await
		} else {
			q.load(db).await
		}
		.map_err(AppError::from)
	}

	/// Archived (soft-deleted) servers, for the Archived view.
	pub async fn list_archived(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_not_null())
			.order_by((kind.asc(), name.asc(), created_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_by_kind(
		db: &mut AsyncPgConnection,
		k: ServerKind,
		offset: u64,
		limit: Option<u64>,
	) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		let q = servers
			.select(Self::as_select())
			.filter(id.ne(Uuid::nil()).and(kind.eq(k)))
			.filter(deleted_at.is_null())
			.order_by((name.is_not_null(), name.asc(), created_at.desc()))
			.offset(offset.try_into().unwrap_or(i64::MAX));

		if let Some(limit) = limit {
			q.limit(limit.try_into().unwrap_or(i64::MAX)).load(db).await
		} else {
			q.load(db).await
		}
		.map_err(AppError::from)
	}

	pub async fn count_all(db: &mut AsyncPgConnection) -> Result<u64> {
		use crate::schema::servers::dsl::*;
		servers
			.count()
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.get_result(db)
			.await
			.map_err(AppError::from)
			.map(|n: i64| n.try_into().unwrap_or_default())
	}

	pub async fn count_by_kind(db: &mut AsyncPgConnection, k: ServerKind) -> Result<u64> {
		use crate::schema::servers::dsl::*;
		servers
			.count()
			.filter(id.ne(Uuid::nil()).and(kind.eq(k)))
			.filter(deleted_at.is_null())
			.get_result(db)
			.await
			.map_err(AppError::from)
			.map(|n: i64| n.try_into().unwrap_or_default())
	}

	pub async fn own(db: &mut AsyncPgConnection) -> Result<Self> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(id.eq(Uuid::nil()))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn all_pingable(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(device_id.is_null().and(id.ne(Uuid::nil())))
			.filter(deleted_at.is_null())
			.filter(host.is_not_null())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		crate::schema::servers::table
			.select(Self::as_select())
			.filter(crate::schema::servers::id.eq(id))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Like [`Server::get_by_id`] but takes a `FOR UPDATE` row lock. A caller
	/// inside a transaction uses this to serialise against concurrent archival
	/// (`soft_delete` locks the same row), closing the archive-vs-register
	/// TOCTOU at enrollment completion.
	pub async fn get_by_id_for_update(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		crate::schema::servers::table
			.select(Self::as_select())
			.filter(crate::schema::servers::id.eq(id))
			.for_update()
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_host(db: &mut AsyncPgConnection, host: String) -> Result<Self> {
		crate::schema::servers::table
			.select(Self::as_select())
			.filter(crate::schema::servers::host.eq(host))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Operator-driven insert. The caller pre-builds the row (id, defaults,
	/// optional URL, optional pre-bound `device_id` for the Tailscale case).
	/// URLs are no longer unique, so there is no collision check.
	pub async fn create(db: &mut AsyncPgConnection, server: Server) -> Result<Self> {
		use crate::schema::servers;

		crate::tags::reject_reserved_keys(&server.tags)?;

		let created = diesel::insert_into(servers::table)
			.values(server)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)?;
		// A new member can change the group's canonical version source.
		recompute_groups(db, [created.group_id]).await?;
		Ok(created)
	}

	/// Archive a server: hide it from live listings and monitoring while
	/// retaining its history. Releases its device (clears `device_id`,
	/// demotes to `Untrusted`, deactivates its keys) so the box can only
	/// return through the gated enrollment flow. Idempotent.
	pub async fn soft_delete(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
		use crate::schema::servers::dsl;
		use diesel_async::AsyncConnection;

		db.transaction::<_, AppError, _>(async |conn| {
			let server: Server = dsl::servers
				.select(Self::as_select())
				.filter(dsl::id.eq(server_id))
				.for_update()
				.first(conn)
				.await
				.map_err(AppError::from)?;

			if server.deleted_at.is_some() {
				return Ok(());
			}

			if let Some(device_id) = server.device_id {
				crate::devices::Device::untrust(conn, device_id).await?;
				crate::devices::Device::deactivate_keys(conn, device_id).await?;
			}

			diesel::update(dsl::servers.filter(dsl::id.eq(server_id)))
				.set((
					dsl::deleted_at
						.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
					dsl::registered_at.eq(None::<jiff_diesel::Timestamp>),
					dsl::device_id.eq(None::<Uuid>),
				))
				.execute(conn)
				.await
				.map_err(AppError::from)?;

			// The server just dropped out of its group's live set, so the
			// group's cached headline version may now belong to someone else.
			recompute_groups(conn, [server.group_id]).await?;
			Ok(())
		})
		.await
	}

	/// Un-archive a server. Does not rebind a device — the box must re-enroll.
	pub async fn restore(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Self> {
		use crate::schema::servers::dsl;

		diesel::update(dsl::servers.filter(dsl::id.eq(server_id)))
			.set(dsl::deleted_at.eq(None::<jiff_diesel::Timestamp>))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		let restored = Self::get_by_id(db, server_id).await?;
		// Back in the live set: the group's canonical member may change.
		recompute_groups(db, [restored.group_id]).await?;
		Ok(restored)
	}

	/// Canonicalise a user-entered URL. A bare host (no scheme) defaults to
	/// `https://`, so operators can type `foo.example.com` and get
	/// `https://foo.example.com`.
	pub fn canonicalize_host(url: &str) -> Result<UrlField> {
		let url = url.trim();
		let candidate = if url.contains("://") {
			url.to_string()
		} else {
			format!("https://{url}")
		};
		Ok(UrlField(candidate.parse().map_err(|e| {
			AppError::BadRequest(format!("Invalid URL: {e}"))
		})?))
	}

	/// Map a hosting hint to the `cloud` flag.
	pub fn detect_cloud(hosting: &str) -> bool {
		matches!(
			hosting,
			"ec2" | "azure" | "gce" | "gcp" | "digitalocean" | "oracle" | "cloudstack"
		)
	}

	/// Live (non-archived) servers currently bound to this device.
	pub async fn live_by_device_id(db: &mut AsyncPgConnection, dev_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(device_id.eq(dev_id))
			.filter(deleted_at.is_null())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Bind a device to a server (sets `device_id`).
	pub async fn bind_device(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		device_id: Uuid,
	) -> Result<()> {
		use crate::schema::servers::dsl;
		diesel::update(dsl::servers.filter(dsl::id.eq(server_id)))
			.set(dsl::device_id.eq(Some(device_id)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Mark a server enrolled (sets `registered_at = now()`).
	pub async fn mark_registered(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
		use crate::schema::servers::dsl;
		diesel::update(dsl::servers.filter(dsl::id.eq(server_id)))
			.set(
				dsl::registered_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
			)
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	pub async fn get_by_device_id(db: &mut AsyncPgConnection, dev_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(device_id.eq(dev_id))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_ids(db: &mut AsyncPgConnection, ids: &[Uuid]) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Servers this device has reported a status for in the past, excluding any
	/// it is currently linked to. Reads from the denormalised
	/// `device_server_associations` table maintained by a trigger on `statuses`.
	pub async fn get_past_associations_for_device(
		db: &mut AsyncPgConnection,
		dev_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::{device_server_associations as a, servers::dsl::*};

		servers
			.inner_join(a::table.on(a::server_id.eq(id)))
			.select(Self::as_select())
			.filter(a::device_id.eq(dev_id))
			.filter(device_id.is_distinct_from(dev_id))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// All servers in the same group as `self`, excluding `self`. If the
	/// server is ungrouped, returns an empty Vec.
	pub async fn siblings(&self, db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		let Some(gid) = self.group_id else {
			return Ok(Vec::new());
		};
		servers
			.select(Self::as_select())
			.filter(group_id.eq(gid))
			.filter(id.ne(self.id))
			.filter(deleted_at.is_null())
			.order(name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// All servers without a group, ordered by name. Used by the Ungrouped UI tab.
	pub async fn list_ungrouped(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(group_id.is_null())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.order(name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn count_ungrouped(db: &mut AsyncPgConnection) -> Result<u64> {
		use crate::schema::servers::dsl::*;
		servers
			.count()
			.filter(group_id.is_null())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.get_result(db)
			.await
			.map_err(AppError::from)
			.map(|n: i64| n.try_into().unwrap_or_default())
	}

	/// Bulk-fetch `(name, host)` for a set of server ids — used by the
	/// issues/incidents APIs to embed display info into each row so the UI
	/// doesn't have to fetch every server independently.
	pub async fn names_by_ids(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, (Option<String>, Option<String>)>> {
		use crate::schema::servers::dsl;

		if ids.is_empty() {
			return Ok(std::collections::HashMap::new());
		}
		let rows: Vec<(Uuid, Option<String>, Option<String>)> = dsl::servers
			.select((dsl::id, dsl::name, dsl::host))
			.filter(dsl::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows.into_iter().map(|(i, n, h)| (i, (n, h))).collect())
	}

	/// Bulk-fetch the group name for each given server. Servers that are
	/// ungrouped (or don't exist) get `None`.
	pub async fn group_names_by_server_ids(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, Option<String>>> {
		use crate::schema::{server_groups, servers};
		use std::collections::HashMap;

		if ids.is_empty() {
			return Ok(HashMap::new());
		}
		let rows: Vec<(Uuid, Option<String>)> = servers::table
			.left_join(server_groups::table.on(server_groups::id.nullable().eq(servers::group_id)))
			.select((servers::id, server_groups::name.nullable()))
			.filter(servers::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows.into_iter().collect())
	}

	/// Bulk-fetch `(group_id, group_name)` for each given server. Servers
	/// that are ungrouped (or don't exist) get `(None, None)`.
	pub async fn group_refs_by_server_ids(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, (Option<Uuid>, Option<String>)>> {
		use crate::schema::{server_groups, servers};
		use std::collections::HashMap;

		if ids.is_empty() {
			return Ok(HashMap::new());
		}
		let rows: Vec<(Uuid, Option<Uuid>, Option<String>)> = servers::table
			.left_join(server_groups::table.on(server_groups::id.nullable().eq(servers::group_id)))
			.select((
				servers::id,
				servers::group_id,
				server_groups::name.nullable(),
			))
			.filter(servers::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows
			.into_iter()
			.map(|(id, gid, gn)| (id, (gid, gn)))
			.collect())
	}

	pub async fn search_central(
		db: &mut AsyncPgConnection,
		query: &str,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		let search_pattern = format!("%{}%", query);

		let mut query_builder = servers
			.select(Self::as_select())
			.filter(kind.eq(ServerKind::Central.to_string()))
			.filter(public_name.is_not_null())
			.filter(deleted_at.is_null())
			.into_boxed();

		if let Ok(query_uuid) = query.parse::<Uuid>() {
			query_builder = query_builder.filter(
				name.ilike(&search_pattern)
					.or(host.ilike(&search_pattern))
					.or(id.eq(query_uuid)),
			);
		} else {
			query_builder =
				query_builder.filter(name.ilike(&search_pattern).or(host.ilike(&search_pattern)));
		}

		query_builder
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn update(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		updates: PartialServer,
	) -> Result<Self> {
		use crate::schema::servers::dsl;

		if let Some(tags) = &updates.tags {
			crate::tags::reject_reserved_keys(tags)?;
		}

		// Capture the old group before the update: rank/kind/group_id may all
		// change, so both the old and new group's canonical member can shift.
		// Non-fatal: a missing server (or read error) just means "no old group
		// to recompute" and leaves the update's own error handling — e.g. the
		// empty-changeset path — to set the response, unchanged by us.
		let old_group_id = Self::get_by_id(db, server_id)
			.await
			.ok()
			.and_then(|s| s.group_id);

		diesel::update(dsl::servers.filter(dsl::id.eq(server_id)))
			.set(updates)
			.execute(db)
			.await
			.map_err(AppError::from)?;

		let after = Self::get_by_id(db, server_id).await?;
		recompute_groups(db, [old_group_id, after.group_id]).await?;
		Ok(after)
	}

	/// Set or clear the server's group. On a `None → Some(group)` transition,
	/// the server's currently-open issues get re-evaluated against the new
	/// group so any that warrant promotion to an incident do so. The clear
	/// case is the simpler direction: the server's open issues stay, but
	/// they no longer contribute to a group-level incident (the existing
	/// incident's other-server contributors keep it alive on their own).
	pub async fn assign_to_group(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		new_group_id: Option<Uuid>,
	) -> Result<Self> {
		use crate::schema::servers::dsl;

		let before = Self::get_by_id(db, server_id).await?;
		diesel::update(dsl::servers.filter(dsl::id.eq(server_id)))
			.set(dsl::group_id.eq(new_group_id))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		let after = Self::get_by_id(db, server_id).await?;

		if before.group_id.is_none() && new_group_id.is_some() {
			// Promote currently-open issues now that this server has somewhere
			// to attach an incident to.
			crate::issues::reevaluate_open_issues_for_server(db, server_id).await?;
		}

		recompute_groups(db, [before.group_id, new_group_id]).await?;

		Ok(after)
	}

	/// Tags as seen by the public-server tags endpoint: the group's tags
	/// with the server's own tags overlaid (server wins on key collision).
	/// If the server is ungrouped, returns just the server's tags.
	pub async fn tags_merged_with_group(&self, db: &mut AsyncPgConnection) -> Result<TagMap> {
		let Some(gid) = self.group_id else {
			return Ok(self.tags.clone());
		};
		let group = crate::server_groups::ServerGroup::get_by_id(db, gid).await?;
		Ok(self.tags.merged_with(&group.tags))
	}

	/// Tags served to the device by the public `/tags` endpoint: the merged
	/// server+group tags (see [`Self::tags_merged_with_group`]) plus synthetic
	/// read-only tags describing the server itself, under the reserved
	/// [`RESERVED_TAG_PREFIX`] namespace:
	///
	/// - `canopy:kind` — the server's [`ServerKind`] (always present).
	/// - `canopy:rank` — the server's [`ServerRank`], only when one is set.
	/// - `canopy:group-id` / `canopy:group-name` — only when grouped.
	///
	/// Operator-set tags can't use the `canopy:` prefix (rejected on write),
	/// so these never collide with stored tags.
	pub async fn tags_for_device(&self, db: &mut AsyncPgConnection) -> Result<TagMap> {
		let group = match self.group_id {
			Some(gid) => Some(crate::server_groups::ServerGroup::get_by_id(db, gid).await?),
			None => None,
		};

		let mut tags = match &group {
			Some(group) => self.tags.merged_with(&group.tags),
			None => self.tags.clone(),
		};

		tags.0
			.insert(format!("{RESERVED_TAG_PREFIX}kind"), self.kind.to_string());
		if let Some(rank) = self.rank {
			tags.0
				.insert(format!("{RESERVED_TAG_PREFIX}rank"), rank.to_string());
		}
		if let Some(group) = &group {
			tags.0.insert(
				format!("{RESERVED_TAG_PREFIX}group-id"),
				group.id.to_string(),
			);
			tags.0.insert(
				format!("{RESERVED_TAG_PREFIX}group-name"),
				group.name.clone(),
			);
		}

		Ok(tags)
	}
}

#[test]
fn canonicalize_host_defaults_to_https() {
	let h = |s: &str| Server::canonicalize_host(s).unwrap().0.to_string();
	assert_eq!(h("foo.example.com"), "https://foo.example.com/");
	assert_eq!(h("  bar.example.com  "), "https://bar.example.com/");
	assert_eq!(h("http://insecure.example"), "http://insecure.example/");
	assert_eq!(h("https://full.example/path"), "https://full.example/path");
}

#[test]
fn test_server_serialization() {
	let server = Server {
		id: Uuid::nil(),
		name: Some("Test Server".to_string()),
		kind: ServerKind::Central,
		rank: Some(ServerRank::Production),
		host: Some(UrlField("https://example.com/".parse().unwrap())),
		device_id: Some(Uuid::nil()),
		group_id: None,
		public_name: Some("Test Server".to_string()),
		cloud: None,
		geolocation: None,
		is_monitored: true,
		allow_legacy_status: false,
		alert_when_down_for: TEN_MINUTES,
		notes: String::new(),
		tags: TagMap::default(),
		deleted_at: None,
		registered_at: None,
	};

	let serialized = serde_json::to_string_pretty(&server).unwrap();
	assert_eq!(
		serialized,
		r#"{
  "id": "00000000-0000-0000-0000-000000000000",
  "name": "Test Server",
  "host": "https://example.com",
  "kind": "central",
  "rank": "production",
  "device_id": "00000000-0000-0000-0000-000000000000",
  "public_name": "Test Server",
  "is_monitored": true,
  "allow_legacy_status": false,
  "alert_when_down_for": 600,
  "notes": "",
  "tags": {}
}"#
	);
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewServer {
	pub name: Option<String>,
	#[serde(default)]
	pub host: Option<UrlField>,
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
	pub device_id: Option<Uuid>,
	#[serde(default)]
	pub group_id: Option<Uuid>,
}

impl From<NewServer> for Server {
	fn from(server: NewServer) -> Self {
		Server {
			id: Uuid::new_v4(),
			name: server.name,
			kind: server.kind,
			rank: server.rank,
			host: server.host,
			device_id: server.device_id,
			group_id: server.group_id,
			public_name: None,
			cloud: None,
			geolocation: None,
			is_monitored: true,
			allow_legacy_status: false,
			alert_when_down_for: TEN_MINUTES,
			notes: String::new(),
			tags: TagMap::default(),
			deleted_at: None,
			registered_at: None,
		}
	}
}

#[derive(Debug, Deserialize, AsChangeset, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PartialServer {
	pub id: Uuid,
	pub name: Option<String>,
	pub kind: Option<ServerKind>,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub rank: Option<ServerRank>,
	/// `Some(Some(url))` sets the URL, `Some(None)` clears it, `None` leaves it.
	pub host: Option<Option<UrlField>>,
	pub device_id: Option<Option<Uuid>>,
	pub group_id: Option<Option<Uuid>>,
	pub public_name: Option<Option<String>>,
	pub cloud: Option<Option<bool>>,
	pub geolocation: Option<Option<GeoPoint>>,
	pub is_monitored: Option<bool>,
	pub allow_legacy_status: Option<bool>,
	#[schema(value_type = Option<i64>)]
	#[diesel(serialize_as = PgDuration)]
	pub alert_when_down_for: Option<PgDuration>,
	pub notes: Option<String>,
	pub tags: Option<TagMap>,
}
