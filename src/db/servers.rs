use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

use super::server_rank::ServerRank;
use super::url_field::UrlField;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::views::ordered_servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Server {
	pub id: Uuid,

	// name and host are required for the public API
	pub name: String,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: UrlField,

	#[diesel(deserialize_as = String, serialize_as = String)]
	pub rank: ServerRank,

	pub device_id: Option<Uuid>,
}

impl Server {
	pub async fn get_all(db: &mut AsyncPgConnection) -> Vec<Self> {
		crate::views::ordered_servers::table
			.select(Server::as_select())
			.load(db)
			.await
			.expect("Error loading servers")
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		crate::views::ordered_servers::table
			.select(Server::as_select())
			.filter(crate::views::ordered_servers::id.eq(id))
			.first(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}
}

#[test]
fn test_server_serialization() {
	let server = Server {
		id: Uuid::nil(),
		name: "Test Server".to_string(),
		rank: ServerRank::Production,
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
  "rank": "production",
  "device_id": "00000000-0000-0000-0000-000000000000"
}"#
	);
}

#[derive(Debug, Deserialize)]
pub struct NewServer {
	pub name: String,
	pub rank: ServerRank,
	pub host: UrlField,
	pub device_id: Uuid,
}

impl From<NewServer> for Server {
	fn from(server: NewServer) -> Self {
		Server {
			id: Uuid::new_v4(),
			name: server.name,
			rank: server.rank,
			host: server.host,
			device_id: Some(server.device_id),
		}
	}
}

#[derive(Debug, Deserialize, AsChangeset)]
#[diesel(table_name = crate::views::ordered_servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PartialServer {
	pub id: Uuid,
	pub name: Option<String>,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub rank: Option<ServerRank>,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: Option<UrlField>,
}
