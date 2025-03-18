use rocket::serde::json::Json;
use rocket_db_pools::{Connection, diesel::prelude::*};

use crate::{app::TamanuHeaders, db::artifacts::Artifact, Db};

#[post("/artifacts", data = "<artifact>")]
pub async fn create(
    mut db: Connection<Db>,
    artifact: Json<Artifact>,
) -> TamanuHeaders<Json<Artifact>> {
    let input = artifact.into_inner();
    diesel::insert_into(crate::schema::artifacts::table)
        .values(input.clone())
        .execute(&mut db)
        .await
        .expect("Error creating artifact");

    TamanuHeaders::new(Json(input))
}
