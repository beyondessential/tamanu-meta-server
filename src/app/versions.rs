use rocket::serde::json::Json;
use rocket_db_pools::Connection;

use crate::{app::TamanuHeaders, db::versions::Version, Db};

#[get("/versions")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Json<Vec<Version>>> {
	let list_of_versions = Version::get_all(&mut db).await;
	TamanuHeaders::new(list_of_versions.into())
}
