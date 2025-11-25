use std::str::FromStr as _;

use axum::{
	Json,
	extract::{Path, State},
	routing::{Router, post},
};
use commons_errors::Result;
use commons_servers::device_auth::ReleaserDevice;
use commons_types::version::{VersionStatus, VersionStr};
use database::{
	Db,
	artifacts::{Artifact, NewArtifact},
	versions::{NewVersion, Version},
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
	let version_str = VersionStr::from_str(&version)?;

	// Try to get the version, or create it as a draft if it doesn't exist
	let version_id = match Version::get_by_version(&mut db, version_str.clone()).await {
		Ok(version) => version.id,
		Err(_) => {
			// Version doesn't exist, create it as a draft
			let new_version = NewVersion {
				major: version_str.0.major as _,
				minor: version_str.0.minor as _,
				patch: version_str.0.patch as _,
				changelog: String::new(),
				status: VersionStatus::Draft,
			};

			let version = diesel::insert_into(database::schema::versions::table)
				.values(new_version)
				.returning(Version::as_select())
				.get_result(&mut db)
				.await?;

			version.id
		}
	};

	let input = NewArtifact {
		version_id,
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
