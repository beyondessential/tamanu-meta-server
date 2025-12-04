use commons_errors::Result;
use commons_types::version::VersionStatus;
use jiff::Timestamp;
use leptos::server;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionData {
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub status: VersionStatus,
	pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinorVersionGroup {
	pub major: i32,
	pub minor: i32,
	pub count: usize,
	pub latest_patch: i32,
	pub first_created_at: Timestamp,
	pub last_created_at: Timestamp,
	pub versions: Vec<VersionData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDetail {
	pub id: Uuid,
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub status: VersionStatus,
	pub created_at: Timestamp,
	pub updated_at: Timestamp,
	pub changelog: String,
	pub min_chrome_version: Option<u32>,
	pub is_latest_in_minor: bool,
	pub related_versions: Vec<RelatedVersionData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedVersionData {
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub changelog: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactData {
	pub id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
}

#[server]
pub async fn get_grouped_versions() -> Result<Vec<MinorVersionGroup>> {
	ssr::get_grouped_versions().await
}

#[server]
pub async fn get_version_detail(version: String) -> Result<VersionDetail> {
	ssr::get_version_detail(version).await
}

#[server]
pub async fn get_version_artifacts(version: String) -> Result<Vec<ArtifactData>> {
	ssr::get_version_artifacts(version).await
}

#[server]
pub async fn get_artifacts_by_version_id(version_id: Uuid) -> Result<Vec<ArtifactData>> {
	ssr::get_artifacts_by_version_id(version_id).await
}

#[server]
pub async fn update_version_status(version: String, status: String) -> Result<()> {
	ssr::update_version_status(version, status).await
}

#[server]
pub async fn update_version_changelog(version: String, changelog: String) -> Result<()> {
	ssr::update_version_changelog(version, changelog).await
}

#[server]
pub async fn update_artifact(
	artifact_id: Uuid,
	artifact_type: String,
	platform: String,
	download_url: String,
) -> Result<()> {
	ssr::update_artifact(artifact_id, artifact_type, platform, download_url).await
}

#[server]
pub async fn create_artifact(
	version_id: Uuid,
	artifact_type: String,
	platform: String,
	download_url: String,
) -> Result<ArtifactData> {
	ssr::create_artifact(version_id, artifact_type, platform, download_url).await
}

#[server]
pub async fn delete_artifact(artifact_id: Uuid) -> Result<()> {
	ssr::delete_artifact(artifact_id).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use std::collections::BTreeMap;
	use std::str::FromStr;

	use axum::extract::State;
	use commons_errors::Result;
	use commons_types::version::{VersionStatus, VersionStr};
	use database::{Db, artifacts::Artifact, versions::Version};
	use jiff::Timestamp;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;

	use crate::state::AppState;

	pub async fn get_grouped_versions() -> Result<Vec<MinorVersionGroup>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let versions = Version::get_all_including_drafts(&mut conn).await?;

		let mut grouped: BTreeMap<(i32, i32), Vec<Version>> = BTreeMap::new();
		for version in versions {
			grouped
				.entry((version.major, version.minor))
				.or_default()
				.push(version);
		}

		let mut result: Vec<MinorVersionGroup> = grouped
			.into_iter()
			.map(|((major, minor), mut versions)| {
				versions.sort_by(|a, b| b.patch.cmp(&a.patch));

				let count = versions.len();

				// Filter to only published versions for calculating latest patch and dates
				let published_versions: Vec<_> = versions
					.iter()
					.filter(|v| v.status == commons_types::version::VersionStatus::Published)
					.collect();

				let latest_patch = published_versions.first().map(|v| v.patch).unwrap_or(0);

				let first_created_at = published_versions
					.iter()
					.find(|v| v.patch == 0)
					.map(|v| v.created_at)
					.unwrap_or_else(|| {
						published_versions
							.last()
							.map(|v| v.created_at)
							.unwrap_or_else(Timestamp::now)
					});

				let last_created_at = published_versions
					.first()
					.map(|v| v.created_at)
					.unwrap_or_else(Timestamp::now);

				let version_data: Vec<VersionData> = versions
					.into_iter()
					.map(|v| VersionData {
						major: v.major,
						minor: v.minor,
						patch: v.patch,
						status: v.status,
						created_at: v.created_at,
					})
					.collect();

				MinorVersionGroup {
					major,
					minor,
					count,
					latest_patch,
					first_created_at,
					last_created_at,
					versions: version_data,
				}
			})
			.collect();

		result.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| b.minor.cmp(&a.minor)));

		Ok(result)
	}

	pub async fn get_version_detail(version_str: String) -> Result<super::VersionDetail> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let version = VersionStr::from_str(&version_str)?;
		let version_record = Version::get_by_version(&mut conn, version.clone()).await?;

		// Compute min chrome version
		let min_chrome_version = if let Ok(head_release_date) =
			Version::get_head_release_date(&mut conn, version.clone()).await
		{
			database::chrome_releases::ChromeRelease::get_min_version_at_date(
				&mut conn,
				head_release_date,
			)
			.await
			.ok()
			.flatten()
		} else {
			None
		};

		// Check if this is the latest published version in its minor
		let is_latest_in_minor = Version::is_latest_in_minor(&mut conn, version.clone())
			.await
			.unwrap_or(true);

		// Get all lower patch versions in this minor release
		let related_versions = Version::get_all_in_minor(&mut conn, version.clone())
			.await
			.unwrap_or_default()
			.into_iter()
			.map(|v| super::RelatedVersionData {
				major: v.major,
				minor: v.minor,
				patch: v.patch,
				changelog: v.changelog,
			})
			.collect();

		Ok(super::VersionDetail {
			id: version_record.id,
			major: version_record.major,
			minor: version_record.minor,
			patch: version_record.patch,
			status: version_record.status,
			created_at: version_record.created_at,
			updated_at: version_record.updated_at,
			changelog: version_record.changelog,
			min_chrome_version,
			is_latest_in_minor,
			related_versions,
		})
	}

	pub async fn get_version_artifacts(version_str: String) -> Result<Vec<super::ArtifactData>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let version = VersionStr::from_str(&version_str)?;
		let version_record = Version::get_by_version(&mut conn, version).await?;
		let artifacts = Artifact::get_for_version(&mut conn, version_record.id).await?;

		Ok(artifacts
			.into_iter()
			.map(|a| super::ArtifactData {
				id: a.id,
				artifact_type: a.artifact_type,
				platform: a.platform,
				download_url: a.download_url,
			})
			.collect())
	}

	pub async fn get_artifacts_by_version_id(version_id: Uuid) -> Result<Vec<super::ArtifactData>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let artifacts = Artifact::get_for_version(&mut conn, version_id).await?;

		Ok(artifacts
			.into_iter()
			.map(|a| super::ArtifactData {
				id: a.id,
				artifact_type: a.artifact_type,
				platform: a.platform,
				download_url: a.download_url,
			})
			.collect())
	}

	pub async fn update_version_status(version_str: String, status_str: String) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let version = VersionStr::from_str(&version_str)?;
		let new_status = VersionStatus::from(status_str);

		// Check if trying to change a published non-latest version to draft
		let version_record = Version::get_by_version(&mut conn, version.clone()).await?;
		if version_record.status == VersionStatus::Published && new_status == VersionStatus::Draft {
			let is_latest = Version::is_latest_in_minor(&mut conn, version.clone()).await?;

			if !is_latest {
				return Err(commons_errors::AppError::custom(
					"Cannot change a published version to draft unless it is the latest in its minor version",
				));
			}
		}

		Version::update_status(&mut conn, version, new_status).await?;

		Ok(())
	}

	pub async fn update_version_changelog(
		version_str: String,
		new_changelog: String,
	) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let version = VersionStr::from_str(&version_str)?;

		Version::update_changelog(&mut conn, version, new_changelog).await?;

		Ok(())
	}

	pub async fn update_artifact(
		artifact_id: Uuid,
		artifact_type: String,
		platform: String,
		download_url: String,
	) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		database::artifacts::Artifact::update(
			&mut conn,
			artifact_id,
			artifact_type,
			platform,
			download_url,
		)
		.await?;

		Ok(())
	}

	pub async fn create_artifact(
		version_id: Uuid,
		artifact_type: String,
		platform: String,
		download_url: String,
	) -> Result<super::ArtifactData> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let artifact = database::artifacts::Artifact::create(
			&mut conn,
			version_id,
			artifact_type,
			platform,
			download_url,
		)
		.await?;

		Ok(super::ArtifactData {
			id: artifact.id,
			artifact_type: artifact.artifact_type,
			platform: artifact.platform,
			download_url: artifact.download_url,
		})
	}

	pub async fn delete_artifact(artifact_id: Uuid) -> Result<()> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		database::artifacts::Artifact::delete(&mut conn, artifact_id).await?;

		Ok(())
	}
}
