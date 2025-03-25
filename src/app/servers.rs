use rocket::serde::json::Json;
use rocket_db_pools::{diesel::prelude::*, Connection};

use crate::{
	app::TamanuHeaders,
	db::{
		devices::{AdminDevice, ServerDevice},
		servers::{NewServer, PartialServer, Server},
		Db,
	},
	error::{AppError, Result},
};

#[get("/servers")]
pub async fn list(mut db: Connection<Db>) -> Result<TamanuHeaders<Json<Vec<Server>>>> {
	Ok(TamanuHeaders::new(Json(Server::get_all(&mut db).await)))
}

#[post("/servers", data = "<input>")]
pub async fn create(
	_device: ServerDevice,
	mut db: Connection<Db>,
	input: Json<NewServer>,
) -> Result<TamanuHeaders<Json<Server>>> {
	let input = input.into_inner();
	let server = Server::from(input);

	let server = diesel::insert_into(crate::views::ordered_servers::table)
		.values(server)
		.on_conflict(crate::views::ordered_servers::host)
		.do_nothing()
		.returning(Server::as_select())
		.get_result(&mut db)
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

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
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(Json(
		ordered_servers
			.filter(id.eq(input_id))
			.select(Server::as_select())
			.first(&mut db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))?,
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
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(()))
}
