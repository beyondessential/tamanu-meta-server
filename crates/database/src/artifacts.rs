use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::versions::Version;

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
	pub device_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Insertable)]
#[diesel(belongs_to(Version))]
#[diesel(table_name = crate::schema::artifacts)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewArtifact {
	pub version_id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
	pub device_id: Option<Uuid>,
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
			.map_err(AppError::from)
	}

	pub async fn update(
		db: &mut AsyncPgConnection,
		artifact_id: Uuid,
		new_type: String,
		new_platform: String,
		new_url: String,
	) -> Result<()> {
		use crate::schema::artifacts::dsl::*;

		diesel::update(artifacts.filter(id.eq(artifact_id)))
			.set((
				artifact_type.eq(new_type),
				platform.eq(new_platform),
				download_url.eq(new_url),
			))
			.execute(db)
			.await?;

		Ok(())
	}

	pub async fn create(
		db: &mut AsyncPgConnection,
		ver_id: Uuid,
		art_type: String,
		plat: String,
		url: String,
	) -> Result<Self> {
		use crate::schema::artifacts::dsl::*;

		let new_artifact = NewArtifact {
			version_id: ver_id,
			artifact_type: art_type,
			platform: plat,
			download_url: url,
			device_id: None,
		};

		diesel::insert_into(artifacts)
			.values(new_artifact)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn delete(db: &mut AsyncPgConnection, artifact_id: Uuid) -> Result<()> {
		use crate::schema::artifacts::dsl::*;

		diesel::delete(artifacts.filter(id.eq(artifact_id)))
			.execute(db)
			.await?;

		Ok(())
	}
}
