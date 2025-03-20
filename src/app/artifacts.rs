use rocket::serde::json::Json;
use rocket_db_pools::{diesel::prelude::*, Connection};

use crate::{
	app::TamanuHeaders,
	db::{artifacts::Artifact, devices::ReleaserDevice},
	error::Result,
	Db,
};

#[post("/artifacts", data = "<artifact>")]
pub async fn create(
	_device: ReleaserDevice,
	mut db: Connection<Db>,
	artifact: Json<Artifact>,
) -> Result<TamanuHeaders<Json<Artifact>>> {
	let input = artifact.into_inner();
	diesel::insert_into(crate::schema::artifacts::table)
		.values(input.clone())
		.execute(&mut db)
		.await
		.expect("Error creating artifact");

	Ok(TamanuHeaders::new(Json(input)))
}
