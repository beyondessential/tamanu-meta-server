use std::fmt::Display;

use rocket::serde::{json::Json, Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use rocket_db_pools::Connection;
use url::Url;
use uuid::Uuid;

use crate::launch::{Db, TamanuHeaders};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerRank {
	Live,
	Demo,
	Dev,
}

#[derive(Debug, Clone, Copy)]
pub struct ServerRankFromStringError;
impl std::error::Error for ServerRankFromStringError {}
impl Display for ServerRankFromStringError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "invalid server rank")
	}
}

impl TryFrom<String> for ServerRank {
	type Error = ServerRankFromStringError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		match value.to_ascii_lowercase().as_ref() {
			"live" => Ok(Self::Live),
			"demo" => Ok(Self::Demo),
			"dev" => Ok(Self::Dev),
			_ => Err(ServerRankFromStringError),
		}
	}
}

impl From<ServerRank> for String {
	fn from(rank: ServerRank) -> Self {
		match rank {
			ServerRank::Live => "live",
			ServerRank::Demo => "demo",
			ServerRank::Dev => "dev",
		}
		.into()
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlField(pub Url);

impl TryFrom<String> for UrlField {
	type Error = url::ParseError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		Ok(Self(Url::parse(&value)?))
	}
}

impl From<UrlField> for String {
	fn from(url: UrlField) -> Self {
		url.0.to_string()
	}
}

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

pub async fn get_servers(db: &mut AsyncPgConnection) -> Vec<Server> {
	crate::schema::servers::table
		.select(Server::as_select())
		.load(db)
		.await
		.expect("Error loading servers")
}

#[get("/servers")]
pub async fn list(mut db: Connection<Db>) -> TamanuHeaders<Json<Vec<Server>>> {
	TamanuHeaders::new(Json(get_servers(&mut db).await))
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

#[post("/servers", data = "<input>")]
pub async fn create(mut db: Connection<Db>, input: Json<NewServer>) -> TamanuHeaders<Json<Server>> {
	let input = input.into_inner();
	let server = Server::from(input);

	diesel::insert_into(crate::schema::servers::table)
		.values(server.clone())
		.execute(&mut db)
		.await
		.expect("Error creating server");

	TamanuHeaders::new(Json(server))
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

#[patch("/servers", data = "<input>")]
pub async fn edit(
	mut db: Connection<Db>,
	input: Json<PartialServer>,
) -> TamanuHeaders<Json<Server>> {
	use crate::schema::servers::dsl::*;

	let input = input.into_inner();
	diesel::update(servers)
		.set(input)
		.execute(&mut db)
		.await
		.expect("Error updating server");

	TamanuHeaders::new(Json(
		servers
			.find(id)
			.select(Server::as_select())
			.first(&mut db)
			.await
			.expect("Error loading server"),
	))
}
