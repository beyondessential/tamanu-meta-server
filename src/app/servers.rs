use rocket::{mtls::Certificate, serde::json::Json};
use rocket_db_pools::{diesel::prelude::*, Connection};

use crate::{
	app::TamanuHeaders,
	db::{
		servers::{NewServer, PartialServer, Server},
		Db,
	},
};

#[get("/servers")]
pub async fn list(mut db: Connection<Db>) -> TamanuHeaders<Json<Vec<Server>>> {
	TamanuHeaders::new(Json(Server::get_all(&mut db).await))
}

#[post("/servers", data = "<input>")]
pub async fn create(
	_auth: Certificate<'_>,
	mut db: Connection<Db>,
	input: Json<NewServer>,
) -> TamanuHeaders<Json<Server>> {
	let input = input.into_inner();
	let server = Server::from(input);

	diesel::insert_into(crate::schema::servers::table)
		.values(server.clone())
		.execute(&mut db)
		.await
		.expect("Error creating server");

	TamanuHeaders::new(Json(server))
}

#[patch("/servers", data = "<input>")]
pub async fn edit(
	_auth: Certificate<'_>,
	mut db: Connection<Db>,
	input: Json<PartialServer>,
) -> TamanuHeaders<Json<Server>> {
	use crate::schema::servers::dsl::*;

	let input = input.into_inner();
	let input_id = input.id;

	diesel::update(servers)
		.filter(id.eq(input_id))
		.set(input)
		.execute(&mut db)
		.await
		.expect("Error updating server");

	TamanuHeaders::new(Json(
		servers
			.filter(id.eq(input_id))
			.select(Server::as_select())
			.first(&mut db)
			.await
			.expect("Error loading server"),
	))
}

#[delete("/servers", data = "<input>")]
pub async fn delete(
	_auth: Certificate<'_>,
	mut db: Connection<Db>,
	input: Json<PartialServer>,
) -> TamanuHeaders<()> {
	use crate::schema::servers::dsl::*;

	let input = input.into_inner();

	diesel::delete(servers)
		.filter(id.eq(input.id))
		.execute(&mut db)
		.await
		.expect("Error deleting server");

	TamanuHeaders::new(())
}
