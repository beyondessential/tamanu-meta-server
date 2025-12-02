use commons_errors::{AppError, Result};
use commons_types::{
	geo::GeoPoint,
	server::{kind::ServerKind, rank::ServerRank},
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::url_field::UrlField;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Server {
	pub id: Uuid,
	pub name: Option<String>,

	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: UrlField,

	#[diesel(deserialize_as = String, serialize_as = String)]
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,
	pub device_id: Option<Uuid>,
	pub parent_server_id: Option<Uuid>,
	pub listed: bool,
	pub cloud: Option<bool>,
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

	pub async fn get_by_device_id(db: &mut AsyncPgConnection, dev_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::servers::dsl::*;
		servers
			.select(Self::as_select())
			.filter(device_id.eq(dev_id))
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize, AsChangeset)]
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
