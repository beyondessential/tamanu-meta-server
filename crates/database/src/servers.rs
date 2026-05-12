use commons_errors::{AppError, Result};
use commons_types::{
	geo::GeoPoint,
	server::{kind::ServerKind, rank::ServerRank, ticket::CanopyTicket},
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::url_field::UrlField;

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, AsChangeset,
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
	pub parent_server_id: Option<Uuid>,
	pub listed: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cloud: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub geolocation: Option<GeoPoint>,
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
	/// but a different ID than `ticket.server_id`.
	pub async fn upsert_from_ticket(
		db: &mut AsyncPgConnection,
		ticket: &CanopyTicket,
		kind: ServerKind,
		rank: Option<ServerRank>,
	) -> Result<Self> {
		let kind = ticket.kind.unwrap_or(kind);
		let rank = ticket.rank.or(rank);
		use crate::schema::servers;

		// Look up parent server by its public key if provided.
		let parent_server_id = if let Some(ref pem) = ticket.central_public_key {
			let central_key_der = CanopyTicket::pem_to_der(pem)?;
			if let Some(central_device) =
				crate::devices::Device::from_key(db, &central_key_der).await?
			{
				Server::get_by_device_id(db, central_device.id)
					.await?
					.into_iter()
					.find(|s| s.kind == ServerKind::Central)
					.map(|s| s.id)
			} else {
				None
			}
		} else {
			None
		};

		// Parse the public key bytes we'll use to find/create the device.
		let key_der = ticket.public_key_der()?;

		// Conflict check: is there already a server with this host but a different ID?
		let existing_by_host = Self::get_by_host(db, ticket.canonical_url.clone()).await;
		match existing_by_host {
			Ok(existing) if existing.id != ticket.server_id => {
				return Err(AppError::custom(format!(
					"A server with host '{}' already exists with a different ID ({})",
					ticket.canonical_url, existing.id,
				)));
			}
			_ => {}
		}

		// Find or create the device that owns this key.
		let device = if let Some(device) = crate::devices::Device::from_key(db, &key_der).await? {
			device
		} else {
			crate::devices::Device::create(db, key_der).await?
		};

		// Build the server value, preserving any existing fields where applicable.
		let host = UrlField(
			ticket
				.canonical_url
				.parse()
				.map_err(|e| AppError::custom(format!("Invalid canonical URL: {e}")))?,
		);

		let cloud = ticket.hosting.as_deref().map(|h| {
			matches!(
				h,
				"ec2" | "azure" | "gce" | "gcp" | "digitalocean" | "oracle" | "cloudstack"
			)
		});

		let server_value = Server {
			id: ticket.server_id,
			name: Some(ticket.hostname.clone()),
			host,
			kind,
			rank,
			device_id: Some(device.id),
			parent_server_id,
			listed: false,
			cloud,
			geolocation: None,
		};

		let host_str = server_value.host.0.to_string();
		let kind_str = server_value.kind.to_string();

		// Upsert: insert or update on conflict.
		diesel::insert_into(servers::table)
			.values((
				servers::id.eq(server_value.id),
				servers::name.eq(&server_value.name),
				servers::host.eq(&host_str),
				servers::kind.eq(&kind_str),
				servers::device_id.eq(server_value.device_id),
				servers::listed.eq(server_value.listed),
			))
			.on_conflict(servers::id)
			.do_update()
			.set((
				servers::name.eq(&server_value.name),
				servers::host.eq(&host_str),
				servers::device_id.eq(server_value.device_id),
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

	pub async fn get_children(&self, db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(parent_server_id.eq(self.id))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Root servers — those without a parent. Each one heads a server group
	/// (the unit used for incident rollup). Ordered by name.
	pub async fn list_roots(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(parent_server_id.is_null())
			.filter(id.ne(Uuid::nil()))
			.order(name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
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

	/// Walks `parent_server_id` upwards from `server_id` and returns the
	/// root of the server group (the server with no parent). When `server_id`
	/// is already a root, returns it unchanged.
	pub async fn root_id(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Uuid> {
		use diesel::sql_types::Uuid as SqlUuid;

		#[derive(QueryableByName)]
		struct RootId {
			#[diesel(sql_type = SqlUuid)]
			id: Uuid,
		}

		let row: RootId = diesel::sql_query(
			"WITH RECURSIVE chain AS (\
				SELECT id, parent_server_id FROM servers WHERE id = $1 \
				UNION ALL \
				SELECT s.id, s.parent_server_id FROM servers s \
					JOIN chain c ON s.id = c.parent_server_id \
			) SELECT id FROM chain WHERE parent_server_id IS NULL LIMIT 1",
		)
		.bind::<SqlUuid, _>(server_id)
		.get_result(db)
		.await?;
		Ok(row.id)
	}

	/// All server ids reachable from `root_id` via `parent_server_id` links,
	/// inclusive of the root itself. A single recursive CTE.
	pub async fn descendant_ids(db: &mut AsyncPgConnection, root_id: Uuid) -> Result<Vec<Uuid>> {
		use diesel::sql_types::Uuid as SqlUuid;

		#[derive(QueryableByName)]
		struct Row {
			#[diesel(sql_type = SqlUuid)]
			id: Uuid,
		}

		let rows: Vec<Row> = diesel::sql_query(
			"WITH RECURSIVE descendants AS (\
				SELECT id FROM servers WHERE id = $1 \
				UNION ALL \
				SELECT s.id FROM servers s \
					JOIN descendants d ON s.parent_server_id = d.id \
			) SELECT id FROM descendants",
		)
		.bind::<SqlUuid, _>(root_id)
		.load(db)
		.await?;
		Ok(rows.into_iter().map(|r| r.id).collect())
	}

	pub async fn search_for_parent(
		db: &mut AsyncPgConnection,
		query: &str,
		current_server_id: Uuid,
		current_rank: Option<ServerRank>,
		current_kind: ServerKind,
	) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		let search_pattern = format!("%{}%", query);
		let mut all_servers = Vec::new();

		if let Ok(query_uuid) = query.parse::<Uuid>()
			&& query_uuid != current_server_id
			&& let Ok(server) = Self::get_by_id(db, query_uuid).await
		{
			all_servers.push(server);
		}

		if all_servers.is_empty() {
			all_servers = servers
				.select(Self::as_select())
				.filter(
					id.ne(current_server_id)
						.and(name.ilike(&search_pattern).or(host.ilike(&search_pattern))),
				)
				.limit(50)
				.load(db)
				.await?;
		}

		all_servers.sort_by_key(|server| {
			let rank_matches = server.rank == current_rank;
			let kind_matches = server.kind == current_kind;

			match (rank_matches, kind_matches) {
				(true, _) => 0,
				(false, false) => 1,
				(false, true) => 2,
			}
		});

		Ok(all_servers)
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
			.filter(listed.eq(true))
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
		parent_server_id: None,
		listed: true,
		cloud: None,
		geolocation: None,
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
  "listed": true
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
			parent_server_id: None,
			listed: false,
			cloud: None,
			geolocation: None,
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
	pub parent_server_id: Option<Option<Uuid>>,
	pub listed: Option<bool>,
	pub cloud: Option<Option<bool>>,
	pub geolocation: Option<Option<GeoPoint>>,
}
