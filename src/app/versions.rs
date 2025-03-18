use rocket::{mtls::Certificate, serde::json::Json};
use rocket_db_pools::{diesel::prelude::*, Connection};
use rocket_dyn_templates::{context, Template};

use crate::{
	app::{TamanuHeaders, Version as ParsedVersion},
	db::{versions::Version, artifacts::Artifact},
	Db,
};

#[get("/versions")]
pub async fn list(mut db: Connection<Db>) -> TamanuHeaders<Json<Vec<Version>>> {
	let versions = Version::get_all(&mut db).await;
	TamanuHeaders::new(Json(versions))
}

#[get("/versions/list")]
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
	_auth: Certificate<'_>,
	mut db: Connection<Db>,
	version: Json<Version>,
) -> TamanuHeaders<Json<Version>> {
	let input = version.into_inner();
	diesel::insert_into(crate::schema::versions::table)
		.values(input.clone())
		.execute(&mut db)
		.await
		.expect("Error creating version");

	TamanuHeaders::new(Json(input))
}

#[delete("/versions/<version>")]
pub async fn delete(
	_auth: Certificate<'_>,
	version: ParsedVersion,
	mut db: Connection<Db>,
) -> TamanuHeaders<()> {

	use crate::schema::versions::dsl::*;

	diesel::delete(versions)
		.filter(major.eq(version.0.major as i32))
		.filter(minor.eq(version.0.minor as i32))
		.filter(patch.eq(version.0.patch as i32))
		.execute(&mut db)
		.await
		.expect("Error deleting version");

	TamanuHeaders::new(())
}

#[get("/versions/<version>/artifacts?<artifact_type>&<platform>", rank = 2)]
pub async fn get_artifacts_for_version(
	version: ParsedVersion,
	artifact_type: Option<String>,
	platform: Option<String>,
	mut db: Connection<Db>,
) -> TamanuHeaders<Json<Vec<Artifact>>> {
	use crate::schema::{artifacts, versions};

	let mut query = artifacts::table
		.inner_join(versions::table)
		.filter(versions::major.eq(version.0.major as i32))
		.filter(versions::minor.eq(version.0.minor as i32))
		.filter(versions::patch.eq(version.0.patch as i32))
		.into_boxed();

	if let Some(atype) = artifact_type {
		query = query.filter(artifacts::artifact_type.eq(atype));
	}

	if let Some(plat) = platform {
		query = query.filter(artifacts::platform.eq(plat));
	}

	let artifacts = query
		.select(Artifact::as_select())
		.load(&mut db)
		.await
		.expect("Error loading artifacts");

	TamanuHeaders::new(Json(artifacts))
}

#[get("/versions/update-for/<version>", rank = 1)]
pub async fn update_for(
	mut db: Connection<Db>,
	version: ParsedVersion,
) -> TamanuHeaders<Json<Vec<Version>>> {
	let updates = diesel::sql_query(
		"WITH ranked_versions AS (
			SELECT *, ROW_NUMBER() OVER (PARTITION BY minor ORDER BY patch DESC) as rn
			FROM versions
			WHERE major = $1
			AND (
				(minor = $2 AND patch > $3) OR
				minor > $2
			)
		)
		SELECT *
		FROM ranked_versions
		WHERE rn = 1
		ORDER BY minor",
	)
	.bind::<diesel::sql_types::Integer, _>(version.0.major as i32)
	.bind::<diesel::sql_types::Integer, _>(version.0.minor as i32)
	.bind::<diesel::sql_types::Integer, _>(version.0.patch as i32)
	.load::<Version>(&mut db)
	.await
	.expect("Error loading versions");

	TamanuHeaders::new(updates.into())
}
