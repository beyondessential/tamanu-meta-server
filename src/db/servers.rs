use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

use super::server_rank::ServerRank;
use super::url_field::UrlField;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Server {
	pub id: Uuid,
	pub name: String,
	#[serde(rename = "type")]
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub rank: ServerRank,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: UrlField,
}

impl Server {
	pub async fn get_all(db: &mut AsyncPgConnection) -> Vec<Self> {
		crate::schema::servers::table
			.select(Server::as_select())
			.load(db)
			.await
			.expect("Error loading servers")
	}
}

#[derive(Debug, Deserialize)]
pub struct NewServer {
	pub name: String,
	#[serde(rename = "type")]
	pub rank: ServerRank,
	pub host: UrlField,
}

impl From<NewServer> for Server {
	fn from(server: NewServer) -> Self {
		Server {
			id: Uuid::new_v4(),
			name: server.name,
			rank: server.rank,
			host: server.host,
		}
	}
}

#[derive(Debug, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::servers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PartialServer {
	pub id: Uuid,
	pub name: Option<String>,
	#[serde(rename = "type")]
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub rank: Option<ServerRank>,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub host: Option<UrlField>,
}
