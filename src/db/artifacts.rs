use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

use crate::db::versions::Version;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, Associations)]
#[diesel(belongs_to(Version))]
#[diesel(table_name = crate::schema::artifacts)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Artifact {
	pub id: Uuid,
	pub version_id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
}

#[derive(Debug, Deserialize)]
pub struct NewArtifact {
	pub version_id: Uuid,
	pub platform: String,
	pub artifact_type: String,
	pub download_url: String,
}

impl From<NewArtifact> for Artifact {
	fn from(artifact: NewArtifact) -> Self {
		Artifact {
			id: Uuid::new_v4(),
			version_id: artifact.version_id,
			platform: artifact.platform,
			artifact_type: artifact.artifact_type,
			download_url: artifact.download_url,
		}
	}
}

impl Artifact {
	pub async fn get_all(db: &mut AsyncPgConnection) -> Vec<Self> {
		use crate::schema::artifacts::*;
		table
			.select(Self::as_select())
			.load(db)
			.await
			.expect("Error loading artifacts")
	}
}
