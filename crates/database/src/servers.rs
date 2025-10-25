use commons_errors::{AppError, Result};
use commons_types::server::{kind::ServerKind, rank::ServerRank};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::url_field::UrlField;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = crate::views::ordered_servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Server {
	pub id: Uuid,
	pub name: Option<String>,

	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: UrlField,

	#[diesel(deserialize_as = String, serialize_as = String)]
	pub kind: ServerKind,
	pub rank: Option<ServerRank>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub device_id: Option<Uuid>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub parent_server_id: Option<Uuid>,
}

impl Server {
	pub async fn get_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::views::ordered_servers::dsl::*;
		ordered_servers
			.select(Self::as_select())
			.filter(id.ne(Uuid::nil()))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn own(db: &mut AsyncPgConnection) -> Result<Self> {
		use crate::views::ordered_servers::dsl::*;
		ordered_servers
			.select(Self::as_select())
			.filter(id.eq(Uuid::nil()))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn all_pingable(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::views::ordered_servers::dsl::*;
		ordered_servers
			.select(Self::as_select())
			.filter(device_id.is_null().and(id.ne(Uuid::nil())))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		crate::views::ordered_servers::table
			.select(Self::as_select())
			.filter(crate::views::ordered_servers::id.eq(id))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_host(db: &mut AsyncPgConnection, host: String) -> Result<Self> {
		crate::views::ordered_servers::table
			.select(Self::as_select())
			.filter(crate::views::ordered_servers::host.eq(host))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_device_id(db: &mut AsyncPgConnection, dev_id: Uuid) -> Result<Vec<Self>> {
		use crate::views::ordered_servers::dsl::*;
		ordered_servers
			.select(Self::as_select())
			.filter(device_id.eq(dev_id))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_children(db: &mut AsyncPgConnection, parent_id: Uuid) -> Result<Vec<Self>> {
		use crate::views::ordered_servers::dsl::*;
		ordered_servers
			.select(Self::as_select())
			.filter(parent_server_id.eq(parent_id))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn search_central(
		db: &mut AsyncPgConnection,
		query: &str,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::views::ordered_servers::dsl::*;
		let search_pattern = format!("%{}%", query);

		let mut query_builder = ordered_servers
			.select(Self::as_select())
			.filter(kind.eq(ServerKind::Central.to_string()))
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
		use crate::views::ordered_servers::dsl;

		diesel::update(dsl::ordered_servers.filter(dsl::id.eq(server_id)))
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
  "device_id": "00000000-0000-0000-0000-000000000000"
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
		}
	}
}

#[derive(Debug, Deserialize, AsChangeset)]
#[diesel(table_name = crate::views::ordered_servers)]
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
}
