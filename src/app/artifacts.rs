use rocket::serde::json::Json;
use rocket_db_pools::{Connection, diesel::prelude::*};

use crate::{app::{TamanuHeaders, Version as ParsedVersion}, db::artifacts::Artifact, Db};

#[get("/artifacts/<version>")]
pub async fn get_for_version(
    version: ParsedVersion,
    mut db: Connection<Db>,
) -> TamanuHeaders<Json<Vec<Artifact>>> {
    use crate::schema::{artifacts, versions};

    let artifacts = artifacts::table
        .inner_join(versions::table)
        .filter(versions::major.eq(version.0.major as i32))
        .filter(versions::minor.eq(version.0.minor as i32))
        .filter(versions::patch.eq(version.0.patch as i32))
        .select(Artifact::as_select())
        .load(&mut db)
        .await
        .expect("Error loading artifacts");

    TamanuHeaders::new(Json(artifacts))
}
