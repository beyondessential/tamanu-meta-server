use rocket::serde::json::Json;
use rocket_db_pools::{diesel::prelude::*, Connection};
use rocket_dyn_templates::{context, Template};

use crate::{
	app::{TamanuHeaders, Version as ParsedVersion},
	db::{
		artifacts::Artifact,
		devices::{AdminDevice, ReleaserDevice},
		versions::{NewVersion, Version},
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
	version: Json<NewVersion>,
) -> Result<TamanuHeaders<Json<Version>>> {
	let input = version.into_inner();
	let version = Version::from(input);
	diesel::insert_into(crate::schema::versions::table)
		.values(version.clone())
		.execute(&mut db)
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(Json(version)))
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

#[cfg(test)]
mod tests {
	use super::*;
	use rocket::{
		http::{ContentType, Status},
		local::blocking::Client,
		routes,
	};
	use rocket_db_pools::Database;

	#[test]
	fn test_version_updates_with_post() {
		let figment = rocket::Config::figment();
		let rocket = rocket::build()
			.configure(figment)
			.attach(Db::init())
			.mount("/", routes![create, update_for]);

		let client = Client::tracked(rocket).expect("valid rocket instance");

		let versions = vec![
			NewVersion {
				major: 2,
				minor: 1,
				patch: 0,
				changelog: "2.1.0 changelog".to_string(),
			},
			NewVersion {
				major: 2,
				minor: 1,
				patch: 1,
				changelog: "2.1.1 changelog".to_string(),
			},
			NewVersion {
				major: 2,
				minor: 2,
				patch: 0,
				changelog: "2.2.0 changelog".to_string(),
			},
			NewVersion {
				major: 2,
				minor: 3,
				patch: 0,
				changelog: "2.3.0 changelog".to_string(),
			},
		];

		for version in versions {
			let response = client
				.post("/versions")
				.header(ContentType::JSON)
				// .header(Header::new("x-client-cert", test_cert.clone()))
				.body(serde_json::to_string(&version).unwrap())
				.dispatch();
			assert_eq!(response.status(), Status::Ok);
		}

		// Test case 1: Latest version (2.3.0) should return empty list
		let response = client.get("/versions/update-for/2.3.0").dispatch();

		assert_eq!(response.status(), Status::Ok);
		let updates: Vec<Version> = serde_json::from_str(&response.into_string().unwrap()).unwrap();
		assert!(updates.is_empty());

		// Test case 2: 2.1.0 should return 2.1.1, 2.2.0, and 2.3.0
		let response = client.get("/versions/update-for/2.1.0").dispatch();

		assert_eq!(response.status(), Status::Ok);
		let updates: Vec<Version> = serde_json::from_str(&response.into_string().unwrap()).unwrap();
		assert!(matches!(updates.as_slice(), [
			Version { major: 2, minor: 1, patch: 1, .. },
			Version { major: 2, minor: 2, patch: 0, .. },
			Version { major: 2, minor: 3, patch: 0, .. },
		]));
	}
}
