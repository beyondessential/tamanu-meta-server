use rocket::serde::json::Json;
use rocket_db_pools::{Connection, diesel::prelude::*};

use crate::{
	Db,
	db::{
		artifacts::{Artifact, NewArtifact},
		versions::Version,
	},
	error::Result,
	servers::device_auth::ReleaserDevice,
};

use super::Version as ParsedVersion;

#[post("/artifacts/<version>/<artifact_type>/<platform>", data = "<url>")]
pub async fn create(
	_device: ReleaserDevice,
	mut db: Connection<Db>,
	version: ParsedVersion,
	artifact_type: String,
	platform: String,
	url: String,
) -> Result<Json<Artifact>> {
	let Version { id, .. } = Version::get_by_version(&mut db, version).await?;

	let input = NewArtifact {
		version_id: id,
		platform,
		artifact_type,
		download_url: url,
	};

	let artifact = diesel::insert_into(crate::schema::artifacts::table)
		.values(input)
		.returning(Artifact::as_select())
		.get_result(&mut db)
		.await?;

	Ok(Json(artifact))
}
