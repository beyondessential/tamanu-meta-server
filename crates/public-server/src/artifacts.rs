use std::str::FromStr as _;

use axum::{
	Json,
	extract::{Path, State},
	routing::{Router, post},
};
use commons_errors::Result;
use commons_servers::device_auth::ReleaserDevice;
use commons_types::version::VersionStr;
use database::{
	Db,
	artifacts::{Artifact, NewArtifact},
	versions::Version,
};
use diesel::SelectableHelper as _;
use diesel_async::RunQueryDsl as _;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
	Router::new().route("/{version}/{artifact_type}/{platform}", post(create))
}

#[axum::debug_handler]
async fn create(
	_device: ReleaserDevice,
	State(db): State<Db>,
	Path((version, artifact_type, platform)): Path<(String, String, String)>,
	url: String,
) -> Result<Json<Artifact>> {
	let mut db = db.get().await?;
	let Version { id, .. } =
		Version::get_by_version(&mut db, VersionStr::from_str(&version)?).await?;

	let input = NewArtifact {
		version_id: id,
		platform,
		artifact_type,
		download_url: url,
	};

	let artifact = diesel::insert_into(database::schema::artifacts::table)
		.values(input)
		.returning(Artifact::as_select())
		.get_result(&mut db)
		.await?;

	Ok(Json(artifact))
}
