use rocket::{mtls::Certificate, serde::json::Json};
use rocket_db_pools::{diesel::prelude::*, Connection};
use rocket_dyn_templates::{context, Template};

use crate::{
	app::{TamanuHeaders, Version as ParsedVersion},
	db::{artifacts::Artifact, versions::Version},
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
		.filter(
			major
				.eq(version.0.major as i32)
				.and(minor.eq(version.0.minor as i32))
				.and(patch.eq(version.0.patch as i32)),
		)
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
		.filter(
			versions::major
				.eq(version.0.major as i32)
				.and(versions::minor.eq(version.0.minor as i32))
				.and(versions::patch.eq(version.0.patch as i32)),
		)
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
	let updates = Version::get_updates_for_version(&mut db, version).await;
	TamanuHeaders::new(Json(updates))
}
