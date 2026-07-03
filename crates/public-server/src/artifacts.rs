use axum::{
	Json,
	extract::{Path, State},
};
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
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

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(create))
}

#[utoipa::path(
	post,
	path = "/{version}/{artifact_type}/{platform}",
	operation_id = "register_artifact",
	tag = "artifacts",
	security(("releaser-device" = [])),
	params(
		("version" = String, Path, description = "Exact semver (e.g. `2.10.5`) or range pattern (e.g. `2.10.x`, `^2.10.0`)."),
		("artifact_type" = String, Path),
		("platform" = String, Path),
	),
	request_body(content = String, description = "Download URL for the artifact, as a plain-text body."),
	responses(
		(status = 200, body = Artifact),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
#[axum::debug_handler]
async fn create(
	device: ReleaserDevice,
	State(db): State<Db>,
	Path((version, artifact_type, platform)): Path<(String, String, String)>,
	url: String,
) -> Result<Json<Artifact>> {
	use node_semver::{Range, Version as SemverVersion};

	let mut db = db.get().await?;
	let device_id = device.0.0.id;

	// Try to parse as a specific version first
	if let Ok(semver) = SemverVersion::parse(&version) {
		// It's a specific version (e.g., "1.0.5")
		let version_str = VersionStr(semver);

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
					device_id: Some(device_id),
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
			version_id: Some(version_id),
			platform,
			artifact_type,
			download_url: url,
			device_id: Some(device_id),
			version_range_pattern: None,
		};

		let artifact = diesel::insert_into(database::schema::artifacts::table)
			.values(input)
			.returning(Artifact::as_select())
			.get_result(&mut db)
			.await?;

		Ok(Json(artifact))
	} else {
		// Try to parse as a range (e.g., "1.0.x", "^1.0.0")
		Range::parse(&version)
			.map_err(|_| commons_errors::AppError::custom("Invalid version or version range"))?;

		let input = NewArtifact {
			version_id: None,
			platform,
			artifact_type,
			download_url: url,
			device_id: Some(device_id),
			version_range_pattern: Some(version),
		};

		let artifact = diesel::insert_into(database::schema::artifacts::table)
			.values(input)
			.returning(Artifact::as_select())
			.get_result(&mut db)
			.await?;

		Ok(Json(artifact))
	}
}
