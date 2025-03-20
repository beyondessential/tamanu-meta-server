use rocket::serde::json::Json;
use rocket_db_pools::{diesel::prelude::*, Connection};
use rocket_dyn_templates::{context, Template};

use crate::{
	app::{TamanuHeaders, Version as ParsedVersion},
	db::{
		artifacts::Artifact,
		devices::{AdminDevice, ReleaserDevice},
		versions::Version,
	},
	error::{AppError, Result},
	Db,
};

#[get("/versions")]
pub async fn list(mut db: Connection<Db>) -> Result<TamanuHeaders<Json<Vec<Version>>>> {
	let versions = Version::get_all(&mut db).await;
	Ok(TamanuHeaders::new(Json(versions)))
}

#[get("/versions/list")]
pub async fn view(mut db: Connection<Db>) -> Result<TamanuHeaders<Template>> {
	let versions = Version::get_all(&mut db).await;
	Ok(TamanuHeaders::new(Template::render(
		"versions",
		context! {
			versions,
		},
	)))
}

#[post("/versions", data = "<version>")]
pub async fn create(
	_device: ReleaserDevice,
	mut db: Connection<Db>,
	version: Json<Version>,
) -> Result<TamanuHeaders<Json<Version>>> {
	let input = version.into_inner();
	diesel::insert_into(crate::schema::versions::table)
		.values(input.clone())
		.execute(&mut db)
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(Json(input)))
}

#[delete("/versions/<version>")]
pub async fn delete(
	_device: AdminDevice,
	version: ParsedVersion,
	mut db: Connection<Db>,
) -> Result<TamanuHeaders<()>> {
	use crate::schema::versions::dsl::*;

	diesel::update(versions)
		.filter(crate::db::versions::predicate_version!(version.0))
		.set(published.eq(false))
		.execute(&mut db)
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(()))
}

#[get("/versions/<version>/artifacts?<artifact_type>&<platform>", rank = 3)]
pub async fn get_artifacts_for_version(
	version: ParsedVersion,
	artifact_type: Option<String>,
	platform: Option<String>,
	mut db: Connection<Db>,
) -> Result<TamanuHeaders<Json<Vec<Artifact>>>> {
	use crate::schema::{artifacts, versions};

	let mut query = artifacts::table
		.inner_join(versions::table)
		.filter(crate::db::versions::predicate_version!(version.0))
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
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(Json(artifacts)))
}

#[get("/versions/<version>/artifacts", rank = 1)]
pub async fn view_artifacts(
	version: ParsedVersion,
	mut db: Connection<Db>,
) -> Result<TamanuHeaders<Template>> {
	let version_clone = version.clone();
	let target_version = Version::get_version_by_id(&mut db, version).await?;
	let artifacts = get_artifacts_for_version(version_clone, None, None, db).await?;

	Ok(TamanuHeaders::new(Template::render(
		"artifacts",
		context! {
			version: target_version,
			artifacts: artifacts.inner.into_inner(),
		},
	)))
}

#[get("/versions/update-for/<version>", rank = 2)]
pub async fn update_for(
	mut db: Connection<Db>,
	version: ParsedVersion,
) -> Result<TamanuHeaders<Json<Vec<Version>>>> {
	let updates = Version::get_updates_for_version(&mut db, version).await;
	Ok(TamanuHeaders::new(Json(updates)))
}
