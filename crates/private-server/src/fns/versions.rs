use std::collections::BTreeMap;
use std::str::FromStr;

use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::version::{VersionStatus, VersionStr};
use database::{artifacts::Artifact, versions::Version};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

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
	/// Whether this artifact is for the exact version (true) or via a range pattern (false)
	pub is_exact: bool,
	/// The version range pattern if this is a ranged artifact, None if exact
	pub version_range_pattern: Option<String>,
	/// If true, this ranged artifact has an exact-version override (only when is_exact=true)
	pub has_range_override: bool,
	/// If true, this is the artifact that will be served to public API clients
	pub is_used_in_public_api: bool,
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/get_grouped_versions", post(get_grouped_versions))
		.route("/get_version_detail", post(get_version_detail))
		.route("/get_version_artifacts", post(get_version_artifacts))
		.route("/update_version_status", post(update_version_status))
		.route("/update_version_changelog", post(update_version_changelog))
		.route("/update_artifact", post(update_artifact))
		.route("/create_artifact", post(create_artifact))
		.route("/delete_artifact", post(delete_artifact))
}

pub async fn get_grouped_versions(
	State(state): State<AppState>,
) -> Result<Json<Vec<MinorVersionGroup>>> {
	let mut conn = state.db.get().await?;
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

			let published_versions: Vec<_> = versions
				.iter()
				.filter(|v| v.status == VersionStatus::Published)
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
	Ok(Json(result))
}

#[derive(Deserialize)]
pub struct VersionStringArgs {
	pub version: String,
}

pub async fn get_version_detail(
	State(state): State<AppState>,
	Json(args): Json<VersionStringArgs>,
) -> Result<Json<VersionDetail>> {
	let mut conn = state.db.get().await?;
	let version = VersionStr::from_str(&args.version)?;
	let version_record = Version::get_by_version(&mut conn, version.clone()).await?;

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

	let is_latest_in_minor = Version::is_latest_in_minor(&mut conn, version.clone())
		.await
		.unwrap_or(true);

	let related_versions = Version::get_all_in_minor(&mut conn, version.clone())
		.await
		.unwrap_or_default()
		.into_iter()
		.map(|v| RelatedVersionData {
			major: v.major,
			minor: v.minor,
			patch: v.patch,
			changelog: v.changelog,
		})
		.collect();

	Ok(Json(VersionDetail {
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
	}))
}

pub async fn get_version_artifacts(
	State(state): State<AppState>,
	Json(args): Json<VersionStringArgs>,
) -> Result<Json<Vec<ArtifactData>>> {
	let mut conn = state.db.get().await?;
	let version = VersionStr::from_str(&args.version)?;
	let version_record = Version::get_by_version(&mut conn, version).await?;
	let artifacts_with_metadata =
		Artifact::get_for_version_with_metadata(&mut conn, version_record.id).await?;
	Ok(Json(
		artifacts_with_metadata
			.into_iter()
			.map(
				|(a, is_exact, has_range_override, is_used_in_public_api)| ArtifactData {
					id: a.id,
					artifact_type: a.artifact_type,
					platform: a.platform,
					download_url: a.download_url,
					is_exact,
					version_range_pattern: a.version_range_pattern,
					has_range_override,
					is_used_in_public_api,
				},
			)
			.collect(),
	))
}

#[derive(Deserialize)]
pub struct UpdateStatusArgs {
	pub version: String,
	pub status: String,
}

pub async fn update_version_status(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<UpdateStatusArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	let version = VersionStr::from_str(&args.version)?;
	let new_status = VersionStatus::from(args.status);

	let version_record = Version::get_by_version(&mut conn, version.clone()).await?;
	if version_record.status == VersionStatus::Published && new_status == VersionStatus::Draft {
		let is_latest = Version::is_latest_in_minor(&mut conn, version.clone()).await?;
		if !is_latest {
			return Err(AppError::custom(
				"Cannot change a published version to draft unless it is the latest in its minor version",
			));
		}
	}

	Version::update_status(&mut conn, version, new_status).await?;
	Ok(Json(()))
}

#[derive(Deserialize)]
pub struct UpdateChangelogArgs {
	pub version: String,
	pub changelog: String,
}

pub async fn update_version_changelog(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<UpdateChangelogArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	let version = VersionStr::from_str(&args.version)?;
	Version::update_changelog(&mut conn, version, args.changelog).await?;
	Ok(Json(()))
}

#[derive(Deserialize)]
pub struct UpdateArtifactArgs {
	pub artifact_id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
}

pub async fn update_artifact(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<UpdateArtifactArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Artifact::update(
		&mut conn,
		args.artifact_id,
		args.artifact_type,
		args.platform,
		args.download_url,
	)
	.await?;
	Ok(Json(()))
}

#[derive(Deserialize)]
pub struct CreateArtifactArgs {
	pub version_id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
}

pub async fn create_artifact(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<CreateArtifactArgs>,
) -> Result<Json<ArtifactData>> {
	let mut conn = state.db.get().await?;
	let artifact = Artifact::create(
		&mut conn,
		args.version_id,
		args.artifact_type,
		args.platform,
		args.download_url,
	)
	.await?;
	Ok(Json(ArtifactData {
		id: artifact.id,
		artifact_type: artifact.artifact_type,
		platform: artifact.platform,
		download_url: artifact.download_url,
		is_exact: true,
		version_range_pattern: None,
		has_range_override: false,
		is_used_in_public_api: true,
	}))
}

#[derive(Deserialize)]
pub struct ArtifactIdArgs {
	pub artifact_id: Uuid,
}

pub async fn delete_artifact(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<ArtifactIdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Artifact::delete(&mut conn, args.artifact_id).await?;
	Ok(Json(()))
}
