use pulldown_cmark::{html, Options, Parser};
use rocket::serde::json::Json;
use rocket_db_pools::{diesel::prelude::*, Connection};
use rocket_dyn_templates::{context, Template};

use crate::{
	app::{TamanuHeaders, Version as ParsedVersion, VersionRange},
	db::{
		artifacts::Artifact,
		devices::{AdminDevice, ReleaserDevice},
		versions::{NewVersion, Version},
	},
	error::{AppError, Result},
	Db,
};

fn parse_markdown(text: &str) -> String {
	let mut options = Options::empty();
	options.insert(Options::ENABLE_FOOTNOTES);
	options.insert(Options::ENABLE_GFM);
	options.insert(Options::ENABLE_SMART_PUNCTUATION);
	options.insert(Options::ENABLE_STRIKETHROUGH);
	options.insert(Options::ENABLE_TABLES);
	let parser = Parser::new_ext(text, options);
	let mut html_output = String::new();
	html::push_html(&mut html_output, parser);
	html_output
}

#[get("/versions")]
pub async fn list(mut db: Connection<Db>) -> Result<TamanuHeaders<Json<Vec<Version>>>> {
	let versions = Version::get_all(&mut db).await?;
	Ok(TamanuHeaders::new(Json(versions)))
}

#[get("/")]
pub async fn view(mut db: Connection<Db>) -> Result<TamanuHeaders<Template>> {
	let mut versions = Version::get_all(&mut db).await?;
	for version in &mut versions {
		version.changelog = parse_markdown(&version.changelog);
	}
	Ok(TamanuHeaders::new(Template::render(
		"versions",
		context! {
			versions,
		},
	)))
}

#[post("/versions/<version>", data = "<changelog>")]
pub async fn create(
	_device: ReleaserDevice,
	mut db: Connection<Db>,
	version: ParsedVersion,
	changelog: String,
) -> Result<TamanuHeaders<Json<Version>>> {
	let version = diesel::insert_into(crate::schema::versions::table)
		.values(NewVersion {
			major: version.0.major as _,
			minor: version.0.minor as _,
			patch: version.0.patch as _,
			changelog,
		})
		.returning(Version::as_select())
		.get_result(&mut db)
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

#[get("/versions/<version>", rank = 1)]
pub async fn view_artifacts(
	version: ParsedVersion,
	mut db: Connection<Db>,
) -> Result<TamanuHeaders<Template>> {
	let mut version = Version::get_by_version(&mut db, version).await?;
	version.changelog = parse_markdown(&version.changelog);
	let artifacts = Artifact::get_for_version(&mut db, version.id).await?;

	Ok(TamanuHeaders::new(Template::render(
		"artifacts",
		context! {
			version,
			artifacts,
		},
	)))
}

#[get("/versions/<version>/artifacts", rank = 1)]
pub async fn get_artifacts(
	version: ParsedVersion,
	mut db: Connection<Db>,
) -> Result<TamanuHeaders<Json<Vec<Artifact>>>> {
	let version = Version::get_by_version(&mut db, version).await?;
	let artifacts = Artifact::get_for_version(&mut db, version.id).await?;

	Ok(TamanuHeaders::new(Json(artifacts)))
}

#[get("/versions/<range>/mobile", rank = 1)]
pub async fn view_mobile_install(
	range: VersionRange,
	mut db: Connection<Db>,
) -> Result<TamanuHeaders<Template>> {
	let version = Version::get_latest_matching(&mut db, range.0).await?;
	let artifacts = Artifact::get_for_version(&mut db, version.id)
		.await?
		.into_iter()
		.filter(|a| a.artifact_type == "mobile")
		.collect::<Vec<_>>();

	Ok(TamanuHeaders::new(Template::render(
		"mobile",
		context! {
			version,
			artifacts,
		},
	)))
}

#[get("/versions/update-for/<version>", rank = 2)]
pub async fn update_for(
	mut db: Connection<Db>,
	version: ParsedVersion,
) -> Result<TamanuHeaders<Json<Vec<Version>>>> {
	let updates = Version::get_updates_for_version(&mut db, version).await?;
	Ok(TamanuHeaders::new(Json(updates)))
}
