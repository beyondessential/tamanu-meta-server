use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

use crate::{
	db::versions::Version,
	error::{AppError, Result},
};

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Associations)]
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

#[derive(Debug, Deserialize, Insertable)]
#[diesel(belongs_to(Version))]
#[diesel(table_name = crate::schema::artifacts)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewArtifact {
	pub version_id: Uuid,
	pub platform: String,
	pub artifact_type: String,
	pub download_url: String,
}

impl Artifact {
	pub async fn get_for_version(db: &mut AsyncPgConnection, version: Uuid) -> Result<Vec<Self>> {
		use crate::schema::artifacts::*;

		table
			.select(Self::as_select())
			.filter(version_id.eq(version))
			.order_by(artifact_type.asc())
			.then_order_by(platform.asc())
			.load(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))
	}
}
