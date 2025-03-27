use rocket::serde::json::Json;
use rocket_db_pools::{diesel::prelude::*, Connection};

use crate::{
	app::TamanuHeaders,
	db::{
		device_role::DeviceRole, devices::{AdminDevice, ServerDevice}, servers::{NewServer, PartialServer, Server}, Db
	},
	error::{AppError, Result},
};

#[get("/servers")]
pub async fn list(mut db: Connection<Db>) -> Result<TamanuHeaders<Json<Vec<Server>>>> {
	Ok(TamanuHeaders::new(Json(
		Server::get_all(&mut db)
			.await?
			.into_iter()
			.map(|mut s| {
				s.device_id = None;
				s
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
	let input = input.into_inner();
	let server = Server::from(input);

	match diesel::insert_into(crate::views::ordered_servers::table)
		.values(server.clone())
		.returning(Server::as_select())
		.get_result(&mut db)
		.await {
			Ok(server) => {
				Ok(TamanuHeaders::new(Json(server)))
			}
			Err(diesel::result::Error::DatabaseError(diesel::result::DatabaseErrorKind::UniqueViolation, _)) => {
				let target_server = Server::get_by_host(&mut db, String::from(server.clone().host)).await?;
				if device.0.role != DeviceRole::Admin && target_server.device_id != Some(device.0.id) {
					return Err(AppError::custom("You are not allowed to edit this server"));
				}
				let updated_server = diesel::update(crate::views::ordered_servers::table)
					.filter(crate::views::ordered_servers::id.eq(target_server.id))
					.set(server)
					.returning(Server::as_select())
					.get_result(&mut db)
					.await
					.map_err(|err| AppError::Database(err.to_string()))?;
				Ok(TamanuHeaders::new(Json(updated_server)))
			}
			Err(err) => {
				Err(AppError::Database(err.to_string()))
			}
		}
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
