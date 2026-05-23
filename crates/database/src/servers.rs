use commons_errors::{AppError, Result};
use commons_types::{
	device::DeviceRole,
	geo::GeoPoint,
	server::{TagMap, kind::ServerKind, rank::ServerRank, ticket::CanopyTicket},
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::pg_duration::PgDuration;
use super::url_field::UrlField;

const TEN_MINUTES: PgDuration = PgDuration(SignedDuration::from_secs(600));

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
#[diesel(table_name = crate::schema::servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Server {
	pub id: Uuid,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,

	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: UrlField,

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

	pub async fn get_by_host(db: &mut AsyncPgConnection, host: String) -> Result<Self> {
		crate::schema::servers::table
			.select(Self::as_select())
			.filter(crate::schema::servers::host.eq(host))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Find or create the device for a ticket, then upsert the server record.
	///
	/// Errors if a server already exists with `ticket.canonical_url` as its host
	/// but a different ID than `ticket.server_id`. Newly-imported servers are
	/// **ungrouped** (`group_id IS NULL`); operators assign a group from the
	/// admin UI after import.
	pub async fn upsert_from_ticket(
		db: &mut AsyncPgConnection,
		ticket: &CanopyTicket,
		kind: ServerKind,
		rank: Option<ServerRank>,
	) -> Result<Self> {
		let kind = ticket.kind.unwrap_or(kind);
		let rank = ticket.rank.or(rank);
		use crate::schema::servers;

		// Parse the public key bytes we'll use to find/create the device.
		let key_der = ticket.public_key_der()?;

		// Build the server value, preserving any existing fields where applicable.
		// Parse the canonical URL *first* so we can canonicalise both the
		// stored host and the host we look up for conflict-detection — a
		// difference in trailing slash, port, or case otherwise produces
		// false "different id" errors.
		let host = UrlField(
			ticket
				.canonical_url
				.parse()
				.map_err(|e| AppError::BadRequest(format!("Invalid canonical URL: {e}")))?,
		);
		let host_str = host.0.to_string();

		// Conflict check: is there already a server with this host but a different ID?
		let existing_by_host = Self::get_by_host(db, host_str.clone()).await;
		match existing_by_host {
			Ok(existing) if existing.id != ticket.server_id => {
				return Err(AppError::Conflict(format!(
					"A server with host '{}' already exists with a different ID ({})",
					host_str, existing.id,
				)));
			}
			_ => {}
		}

		// Find or create the device that owns this key. The ticket is
		// the operator's trust signal — promote a freshly-created (or
		// previously-Untrusted) device to `Server` so they don't have
		// to flip it manually after import.
		let device = if let Some(device) = crate::devices::Device::from_key(db, &key_der).await? {
			device
		} else {
			crate::devices::Device::create(db, key_der).await?
		};
		if device.role == DeviceRole::Untrusted {
			crate::devices::Device::trust(db, device.id, DeviceRole::Server).await?;
		}

		let cloud = ticket.hosting.as_deref().map(|h| {
			matches!(
				h,
				"ec2" | "azure" | "gce" | "gcp" | "digitalocean" | "oracle" | "cloudstack"
			)
		});

		let kind_str = kind.to_string();
		let rank_str = rank.map(|r| r.to_string());

		// Upsert: insert or update on conflict. Ticket-derived fields
		// (kind, rank, cloud) re-apply on update; operator-edited state
		// (public_name, geolocation, alert_when_down_for, group_id, notes,
		// tags) is preserved.
		diesel::insert_into(servers::table)
			.values((
				servers::id.eq(ticket.server_id),
				servers::name.eq(&Some(ticket.hostname.clone())),
				servers::host.eq(&host_str),
				servers::kind.eq(&kind_str),
				servers::rank.eq(&rank_str),
				servers::device_id.eq(Some(device.id)),
				servers::cloud.eq(cloud),
			))
			.on_conflict(servers::id)
			.do_update()
			.set((
				servers::name.eq(&Some(ticket.hostname.clone())),
				servers::host.eq(&host_str),
				servers::kind.eq(&kind_str),
				servers::rank.eq(&rank_str),
				servers::device_id.eq(Some(device.id)),
				servers::cloud.eq(cloud),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
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
	) -> Result<std::collections::HashMap<Uuid, (Option<String>, String)>> {
		use crate::schema::servers::dsl;

		if ids.is_empty() {
			return Ok(std::collections::HashMap::new());
		}
		let rows: Vec<(Uuid, Option<String>, String)> = dsl::servers
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

		diesel::update(dsl::servers.filter(dsl::id.eq(server_id)))
			.set(updates)
			.execute(db)
			.await
			.map_err(AppError::from)?;

		Self::get_by_id(db, server_id).await
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
}

#[test]
fn test_server_serialization() {
	let server = Server {
		id: Uuid::nil(),
		name: Some("Test Server".to_string()),
		kind: ServerKind::Central,
		rank: Some(ServerRank::Production),
		host: UrlField("https://example.com/".parse().unwrap()),
		device_id: Some(Uuid::nil()),
		group_id: None,
		public_name: Some("Test Server".to_string()),
		cloud: None,
		geolocation: None,
		is_monitored: true,
		alert_when_down_for: TEN_MINUTES,
		notes: String::new(),
		tags: TagMap::default(),
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
  "alert_when_down_for": 600,
  "notes": "",
  "tags": {}
}"#
	);
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewServer {
	pub name: Option<String>,
	pub host: UrlField,
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
			alert_when_down_for: TEN_MINUTES,
			notes: String::new(),
			tags: TagMap::default(),
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
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: Option<UrlField>,
	pub device_id: Option<Option<Uuid>>,
	pub group_id: Option<Option<Uuid>>,
	pub public_name: Option<Option<String>>,
	pub cloud: Option<Option<bool>>,
	pub geolocation: Option<Option<GeoPoint>>,
	pub is_monitored: Option<bool>,
	#[schema(value_type = Option<i64>)]
	#[diesel(serialize_as = PgDuration)]
	pub alert_when_down_for: Option<PgDuration>,
	pub notes: Option<String>,
	pub tags: Option<TagMap>,
}
