use rocket::serde::json::Json;
use rocket_db_pools::Connection;

use crate::{app::{TamanuHeaders, Version as ParsedVersion}, db::versions::Version, Db};

#[get("/versions")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Json<Vec<Version>>> {
	let list_of_versions = Version::get_all(&mut db).await;
	TamanuHeaders::new(list_of_versions.into())
}

#[get("/versions/update-for/<version>")]
pub async fn update_for(mut db: Connection<Db>, version: ParsedVersion) -> TamanuHeaders<Json<Vec<Version>>> {
	let major = version.0.major;
	let list_of_versions = Version::get_all(&mut db).await;
	TamanuHeaders::new(list_of_versions.into())
}
