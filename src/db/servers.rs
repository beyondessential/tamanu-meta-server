use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

use super::server_kind::ServerKind;
use super::server_rank::ServerRank;
use super::url_field::UrlField;

use crate::error::{AppError, Result};

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
}

impl Server {
	pub async fn get_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		crate::views::ordered_servers::table
			.select(Server::as_select())
			.load(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub async fn all_pingable(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::views::ordered_servers::dsl::*;
		ordered_servers
			.select(Server::as_select())
			.filter(device_id.is_null())
			.load(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		crate::views::ordered_servers::table
			.select(Server::as_select())
			.filter(crate::views::ordered_servers::id.eq(id))
			.first(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}
	pub async fn get_by_host(db: &mut AsyncPgConnection, host: String) -> Result<Self> {
		crate::views::ordered_servers::table
			.select(Server::as_select())
			.filter(crate::views::ordered_servers::host.eq(host))
			.first(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
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
}
