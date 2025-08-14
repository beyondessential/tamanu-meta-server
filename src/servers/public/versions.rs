use std::{str::FromStr as _, sync::Arc};

use axum::{
	Json,
	body::Bytes,
	extract::{Path, State},
	response::Html,
	routing::{Router, delete, get, post},
};
use diesel::{ExpressionMethods as _, SelectableHelper as _};
use diesel_async::RunQueryDsl as _;
use futures::AsyncReadExt;
use pulldown_cmark::{Options, Parser, html};
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use tera::{Context, Tera};

use crate::{
	db::{
		artifacts::Artifact,
		versions::{NewVersion, Version},
	},
	error::Result,
	servers::{
		device_auth::{AdminDevice, ReleaserDevice},
		version::{VersionRange, VersionStr},
	},
	state::{AppState, Db},
};

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/", get(list))
		.route("/update-for/{version}", get(update_for))
		.route("/{version}", post(create))
		.route("/{version}", delete(remove))
		.route("/{version}", get(view_artifacts))
		.route("/{version}/artifacts", get(list_artifacts))
		.route("/{version}/mobile", get(view_mobile_install))
}

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

pub fn parse_markdown(text: &str) -> String {
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

async fn list(State(db): State<Db>) -> Result<Json<Vec<Version>>> {
	let mut db = db.get().await?;
	let versions = Version::get_all(&mut db).await?;
	Ok(Json(versions))
}

async fn create(
	_device: ReleaserDevice,
	Path(version): Path<String>,
	State(db): State<Db>,
	data: Bytes,
) -> Result<Json<Version>> {
	let mut db = db.get().await?;
	let mut stream = data.take(1024 * 1024 * 1024); // up to a MiB
	let mut changelog = String::with_capacity(data.len().min(1024 * 1024 * 1024));
	stream.read_to_string(&mut changelog).await?;
	let version = VersionStr::from_str(&version)?;
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

async fn remove(
	_device: AdminDevice,
	Path(version): Path<String>,
	State(db): State<Db>,
) -> Result<()> {
	use crate::schema::versions::dsl::*;

	let mut db = db.get().await?;
	let version = VersionStr::from_str(&version)?;
	diesel::update(versions)
		.filter(crate::db::versions::predicate_version!(version.0))
		.set(published.eq(false))
		.execute(&mut db)
		.await?;

	Ok(())
}

async fn view_artifacts(
	Path(version): Path<String>,
	State(db): State<Db>,
	State(tera): State<Arc<Tera>>,
) -> Result<Html<String>> {
	let mut db = db.get().await?;
	let version = VersionRange::from_str(&version)?;
	let mut version = Version::get_latest_matching(&mut db, version.0).await?;
	version.changelog = parse_markdown(&version.changelog);
	let artifacts = Artifact::get_for_version(&mut db, version.id).await?;

	let mut context = Context::new();
	context.insert("version", &version);
	context.insert("artifacts", &artifacts);
	Ok(Html(tera.render("artifacts", &context)?))
}

async fn list_artifacts(
	Path(version): Path<String>,
	State(db): State<Db>,
) -> Result<Json<Vec<Artifact>>> {
	let mut db = db.get().await?;
	let version = VersionRange::from_str(&version)?;
	let version = Version::get_latest_matching(&mut db, version.0).await?;
	let artifacts = Artifact::get_for_version(&mut db, version.id).await?;

	Ok(Json(artifacts))
}

async fn view_mobile_install(
	Path(version): Path<String>,
	State(db): State<Db>,
	State(tera): State<Arc<Tera>>,
) -> Result<Html<String>> {
	let mut db = db.get().await?;
	let version = VersionRange::from_str(&version)?;
	let version = Version::get_latest_matching(&mut db, version.0).await?;
	let artifacts = Artifact::get_for_version(&mut db, version.id)
		.await?
		.into_iter()
		.filter(|a| a.artifact_type == "mobile")
		.map(ArtifactWithQR::from)
		.collect::<Vec<_>>();

	let mut context = Context::new();
	context.insert("version", &version);
	context.insert("artifacts", &artifacts);
	Ok(Html(tera.render("mobile", &context)?))
}

async fn update_for(
	State(db): State<Db>,
	Path(version): Path<String>,
) -> Result<Json<Vec<Version>>> {
	let mut db = db.get().await?;
	let version = VersionStr::from_str(&version)?;
	let updates = Version::get_updates_for_version(&mut db, version).await?;
	Ok(Json(updates))
}
