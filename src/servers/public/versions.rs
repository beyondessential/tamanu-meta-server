use pulldown_cmark::{Options, Parser, html};
use qrcode::{QrCode, render::svg};
use rocket::{
	data::{Data, ToByteUnit},
	serde::{Deserialize, Serialize, json::Json},
	tokio::io::AsyncReadExt,
};
use rocket_db_pools::{Connection, diesel::prelude::*};
use rocket_dyn_templates::{Template, context};

use crate::{
	Db,
	db::{
		artifacts::Artifact,
		versions::{NewVersion, Version},
	},
	error::Result,
	servers::{
		device_auth::{AdminDevice, ReleaserDevice},
		version::{Version as ParsedVersion, VersionRange},
	},
};

// Add a derived struct for Artifact with QR code
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactWithQR {
	#[serde(flatten)]
	artifact: Artifact,
	qr_code_svg: String,
}

impl From<Artifact> for ArtifactWithQR {
	fn from(artifact: Artifact) -> Self {
		let code = QrCode::new(&artifact.download_url).expect("Failed to generate QR code");
		let svg_image = code
			.render::<svg::Color>()
			.min_dimensions(100, 100)
			.dark_color(svg::Color("#000000"))
			.light_color(svg::Color("#ffffff"))
			.build();

		Self {
			artifact,
			qr_code_svg: svg_image,
		}
	}
}

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
pub async fn list(mut db: Connection<Db>) -> Result<Json<Vec<Version>>> {
	let versions = Version::get_all(&mut db).await?;
	Ok(Json(versions))
}

#[get("/")]
pub async fn view(mut db: Connection<Db>) -> Result<Template> {
	let mut versions = Version::get_all(&mut db).await?;
	for version in &mut versions {
		version.changelog = parse_markdown(&version.changelog);
	}
	let env = std::env::vars().collect::<std::collections::BTreeMap<String, String>>();
	Ok(Template::render(
		"versions",
		context! {
			versions,
			env,
		},
	))
}

#[post("/versions/<version>", data = "<data>")]
pub async fn create(
	_device: ReleaserDevice,
	mut db: Connection<Db>,
	version: ParsedVersion,
	data: Data<'_>,
) -> Result<Json<Version>> {
	let mut stream = data.open(1_u8.mebibytes());
	let mut changelog = String::with_capacity(stream.hint());
	stream.read_to_string(&mut changelog).await?;
	let version = diesel::insert_into(crate::schema::versions::table)
		.values(NewVersion {
			major: version.0.major as _,
			minor: version.0.minor as _,
			patch: version.0.patch as _,
			changelog,
		})
		.returning(Version::as_select())
		.get_result(&mut db)
		.await?;

	Ok(Json(version))
}

#[delete("/versions/<version>")]
pub async fn delete(
	_device: AdminDevice,
	version: ParsedVersion,
	mut db: Connection<Db>,
) -> Result<()> {
	use crate::schema::versions::dsl::*;

	diesel::update(versions)
		.filter(crate::db::versions::predicate_version!(version.0))
		.set(published.eq(false))
		.execute(&mut db)
		.await?;

	Ok(())
}

#[get("/versions/<version>", rank = 1)]
pub async fn view_artifacts(version: VersionRange, mut db: Connection<Db>) -> Result<Template> {
	let mut version = Version::get_latest_matching(&mut db, version.0).await?;
	version.changelog = parse_markdown(&version.changelog);
	let artifacts = Artifact::get_for_version(&mut db, version.id).await?;

	Ok(Template::render(
		"artifacts",
		context! {
			version,
			artifacts,
		},
	))
}

#[get("/versions/<version>/artifacts", rank = 1)]
pub async fn get_artifacts(
	version: VersionRange,
	mut db: Connection<Db>,
) -> Result<Json<Vec<Artifact>>> {
	let version = Version::get_latest_matching(&mut db, version.0).await?;
	let artifacts = Artifact::get_for_version(&mut db, version.id).await?;

	Ok(Json(artifacts))
}

#[get("/versions/<version>/mobile", rank = 1)]
pub async fn view_mobile_install(
	version: VersionRange,
	mut db: Connection<Db>,
) -> Result<Template> {
	let version = Version::get_latest_matching(&mut db, version.0).await?;
	let artifacts = Artifact::get_for_version(&mut db, version.id)
		.await?
		.into_iter()
		.filter(|a| a.artifact_type == "mobile")
		.map(ArtifactWithQR::from)
		.collect::<Vec<_>>();

	Ok(Template::render(
		"mobile",
		context! {
			version,
			artifacts,
		},
	))
}

#[get("/versions/update-for/<version>", rank = 2)]
pub async fn update_for(
	mut db: Connection<Db>,
	version: ParsedVersion,
) -> Result<Json<Vec<Version>>> {
	let updates = Version::get_updates_for_version(&mut db, version).await?;
	Ok(Json(updates))
}
