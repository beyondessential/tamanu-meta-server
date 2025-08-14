use rocket::serde::{Serialize, json::Json};
use rocket_db_pools::{Connection, diesel::prelude::*};

use crate::{
	db::{
		Db,
		server_kind::ServerKind,
		server_rank::ServerRank,
		servers::{NewServer, PartialServer, Server},
		url_field::UrlField,
	},
	error::Result,
	servers::{
		device_auth::{AdminDevice, ServerDevice},
		headers::TamanuHeaders,
	},
};

#[derive(Debug, Serialize)]
pub struct PublicServer {
	pub name: String,
	pub host: UrlField,
	pub rank: Option<ServerRank>,
}

#[get("/servers")]
pub async fn list(mut db: Connection<Db>) -> Result<TamanuHeaders<Json<Vec<PublicServer>>>> {
	Ok(TamanuHeaders::new(Json(
		Server::get_all(&mut db)
			.await?
			.into_iter()
			.filter_map(|s| {
				(s.kind == ServerKind::Central)
					.then(|| {
						s.name.map(|name| PublicServer {
							name,
							host: s.host,
							rank: s.rank,
						})
					})
					.flatten()
			})
			.collect(),
	)))
}

#[post("/servers", data = "<input>")]
pub async fn create(
	device: ServerDevice,
	mut db: Connection<Db>,
	input: Json<NewServer>,
) -> Result<TamanuHeaders<Json<Server>>> {
	let mut input = Server::from(input.into_inner());
	input.device_id = Some(device.0.id);

	let server = diesel::insert_into(crate::views::ordered_servers::table)
		.values(input)
		.returning(Server::as_select())
		.get_result(&mut db)
		.await?;

	Ok(TamanuHeaders::new(Json(server)))
}

#[patch("/servers", data = "<input>")]
pub async fn edit(
	_device: ServerDevice,
	mut db: Connection<Db>,
	input: Json<PartialServer>,
) -> Result<TamanuHeaders<Json<Server>>> {
	use crate::views::ordered_servers::dsl::*;

	let input = input.into_inner();
	let input_id = input.id;

	diesel::update(ordered_servers)
		.filter(id.eq(input_id))
		.set(input)
		.execute(&mut db)
		.await?;

	Ok(TamanuHeaders::new(Json(
		ordered_servers
			.filter(id.eq(input_id))
			.select(Server::as_select())
			.first(&mut db)
			.await?,
	)))
}

#[delete("/servers", data = "<input>")]
pub async fn delete(
	_device: AdminDevice,
	mut db: Connection<Db>,
	input: Json<PartialServer>,
) -> Result<TamanuHeaders<()>> {
	use crate::schema::servers::dsl::*;

	let input = input.into_inner();

	diesel::delete(servers)
		.filter(id.eq(input.id))
		.execute(&mut db)
		.await?;

	Ok(TamanuHeaders::new(()))
}
