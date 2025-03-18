use rocket::serde::json::Json;
use rocket_db_pools::{diesel::prelude::*, Connection};
use rocket_dyn_templates::{context, Template};

use crate::{
	app::{TamanuHeaders, Version as ParsedVersion},
	db::versions::Version,
	Db,
};

#[get("/versions")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Template> {
	let versions = Version::get_all(&mut db).await;
	TamanuHeaders::new(Template::render(
		"versions",
		context! {
			versions,
		},
	))
}

#[post("/versions", data = "<version>")]
pub async fn create(
	mut db: Connection<Db>,
	version: Json<Version>,
) -> TamanuHeaders<Json<Version>> {
	let input = version.into_inner();
	let version = Version::from(input);
	diesel::insert_into(crate::schema::versions::table)
		.values(version.clone())
		.execute(&mut db)
		.await
		.expect("Error creating version");

	TamanuHeaders::new(Json(version))
}


#[get("/versions/update-for/<version>")]
pub async fn update_for(
	mut db: Connection<Db>,
	version: ParsedVersion,
) -> TamanuHeaders<Json<Vec<Version>>> {
	let target_major = version.0.major as i32;
	let target_minor = version.0.minor as i32;

	let updates = diesel::sql_query(
		"WITH ranked_versions AS (
			SELECT *, ROW_NUMBER() OVER (PARTITION BY minor ORDER BY patch DESC) as rn
			FROM versions
			WHERE major = $1 AND (minor = $2 OR minor > $2)
		)
		SELECT id, major, minor, patch, published
		FROM ranked_versions
		WHERE rn = 1
		ORDER BY minor",
	)
	.bind::<diesel::sql_types::Integer, _>(target_major)
	.bind::<diesel::sql_types::Integer, _>(target_minor)
	.load::<Version>(&mut db)
	.await
	.expect("Error loading versions");

	TamanuHeaders::new(updates.into())
}