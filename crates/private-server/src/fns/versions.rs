use std::collections::BTreeMap;
use std::str::FromStr;

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::{TailscaleAdmin, TailscaleUser};
use commons_types::version::{VersionStatus, VersionStr};
use database::{artifacts::Artifact, version_known_issues::VersionKnownIssue, versions::Version};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VersionData {
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub status: VersionStatus,
	pub created_at: Timestamp,
	/// `true` when this version has no unresolved known issues. See the
	/// `version_known_issues` table and `add_known_issue`/`resolve_known_issue`
	/// endpoints for management.
	pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MinorVersionGroup {
	pub major: i32,
	pub minor: i32,
	pub count: usize,
	pub latest_patch: i32,
	pub first_created_at: Timestamp,
	pub last_created_at: Timestamp,
	pub versions: Vec<VersionData>,
	/// `true` when the latest published patch in this minor is itself
	/// ready. An old, since-fixed issue on an earlier patch doesn't
	/// dim the whole minor.
	pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
	/// `true` when this version has no unresolved known issues.
	pub ready: bool,
	pub known_issues: Vec<KnownIssueData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnownIssueData {
	pub id: Uuid,
	pub created_at: Timestamp,
	pub author: String,
	pub description: String,
	pub min_major: i32,
	pub min_minor: i32,
	pub min_patch: i32,
	/// First unaffected patch. NULL while open — open issues
	/// implicitly cover every patch from `min` to the end of the minor.
	pub max_major: Option<i32>,
	pub max_minor: Option<i32>,
	pub max_patch: Option<i32>,
	pub resolved_at: Option<Timestamp>,
	pub resolved_by: Option<String>,
	pub resolution_message: Option<String>,
}

impl From<VersionKnownIssue> for KnownIssueData {
	fn from(k: VersionKnownIssue) -> Self {
		Self {
			id: k.id,
			created_at: k.created_at,
			author: k.author,
			description: k.description,
			min_major: k.min_major,
			min_minor: k.min_minor,
			min_patch: k.min_patch,
			max_major: k.max_major,
			max_minor: k.max_minor,
			max_patch: k.max_patch,
			resolved_at: k.resolved_at,
			resolved_by: k.resolved_by,
			resolution_message: k.resolution_message,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RelatedVersionData {
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub changelog: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(get_grouped_versions))
		.routes(routes!(get_version_detail))
		.routes(routes!(get_version_artifacts))
		.routes(routes!(update_version_status))
		.routes(routes!(update_version_changelog))
		.routes(routes!(update_artifact))
		.routes(routes!(create_artifact))
		.routes(routes!(delete_artifact))
		.routes(routes!(list_known_issues))
		.routes(routes!(add_known_issue))
		.routes(routes!(resolve_known_issue))
}

#[utoipa::path(
	post,
	path = "/get_grouped_versions",
	tag = "versions",
	responses(
		(status = 200, body = Vec<MinorVersionGroup>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn get_grouped_versions(
	State(state): State<AppState>,
) -> Result<Json<Vec<MinorVersionGroup>>> {
	let mut conn = state.db.get().await?;
	let versions = Version::get_all_including_drafts(&mut conn).await?;

	// One batched query to compute `ready` for every version returned.
	let version_ids: Vec<Uuid> = versions.iter().map(|v| v.id).collect();
	let affected = VersionKnownIssue::affected_versions(&mut conn, &version_ids).await?;

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
			// The minor is "ready" iff its latest published patch is itself
			// ready. If no patches are published, treat the minor as ready
			// — there's nothing for users to receive yet.
			let ready = published_versions
				.first()
				.map(|v| !affected.contains(&v.id))
				.unwrap_or(true);

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
					ready: !affected.contains(&v.id),
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
				ready,
			}
		})
		.collect();

	result.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| b.minor.cmp(&a.minor)));
	Ok(Json(result))
}

#[derive(Deserialize, ToSchema)]
pub struct VersionStringArgs {
	pub version: String,
}

#[utoipa::path(
	post,
	path = "/get_version_detail",
	tag = "versions",
	request_body = VersionStringArgs,
	responses(
		(status = 200, body = VersionDetail),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
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

	// Surface every issue ever raised against this minor (resolved or
	// not), so the operator sees the full timeline of caveats for the
	// branch.
	let known_issues: Vec<KnownIssueData> =
		VersionKnownIssue::list_for_minor(&mut conn, version_record.major, version_record.minor)
			.await?
			.into_iter()
			.map(KnownIssueData::from)
			.collect();
	// `ready` is whether THIS exact patch is unaffected — older issues
	// fixed below this patch don't affect us.
	let ready = VersionKnownIssue::version_is_ready(
		&mut conn,
		version_record.major,
		version_record.minor,
		version_record.patch,
	)
	.await?;

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
		ready,
		known_issues,
	}))
}

#[utoipa::path(
	post,
	path = "/get_version_artifacts",
	tag = "versions",
	request_body = VersionStringArgs,
	responses(
		(status = 200, body = Vec<ArtifactData>),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
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

#[derive(Deserialize, ToSchema)]
pub struct UpdateStatusArgs {
	pub version: String,
	pub status: String,
}

#[utoipa::path(
	post,
	path = "/update_version_status",
	tag = "versions",
	security(("tailscale-admin" = [])),
	request_body = UpdateStatusArgs,
	responses(
		(status = 200),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn update_version_status(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
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

#[derive(Deserialize, ToSchema)]
pub struct UpdateChangelogArgs {
	pub version: String,
	pub changelog: String,
}

#[utoipa::path(
	post,
	path = "/update_version_changelog",
	tag = "versions",
	security(("tailscale-admin" = [])),
	request_body = UpdateChangelogArgs,
	responses(
		(status = 200),
	),
)]
pub async fn update_version_changelog(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<UpdateChangelogArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	let version = VersionStr::from_str(&args.version)?;
	Version::update_changelog(&mut conn, version, args.changelog).await?;
	Ok(Json(()))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateArtifactArgs {
	pub artifact_id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
}

#[utoipa::path(
	post,
	path = "/update_artifact",
	tag = "versions",
	security(("tailscale-admin" = [])),
	request_body = UpdateArtifactArgs,
	responses(
		(status = 200),
	),
)]
pub async fn update_artifact(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
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

#[derive(Deserialize, ToSchema)]
pub struct CreateArtifactArgs {
	pub version_id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
}

#[utoipa::path(
	post,
	path = "/create_artifact",
	tag = "versions",
	security(("tailscale-admin" = [])),
	request_body = CreateArtifactArgs,
	responses(
		(status = 200, body = ArtifactData),
	),
)]
pub async fn create_artifact(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
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

#[derive(Deserialize, ToSchema)]
pub struct ArtifactIdArgs {
	pub artifact_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/delete_artifact",
	tag = "versions",
	security(("tailscale-admin" = [])),
	request_body = ArtifactIdArgs,
	responses(
		(status = 200),
	),
)]
pub async fn delete_artifact(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ArtifactIdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Artifact::delete(&mut conn, args.artifact_id).await?;
	Ok(Json(()))
}

#[derive(Deserialize, ToSchema)]
pub struct VersionIdArgs {
	pub version_id: Uuid,
}

#[utoipa::path(
	post,
	path = "/list_known_issues",
	tag = "versions",
	security(("tailscale-user" = [])),
	request_body = VersionIdArgs,
	responses(
		(status = 200, body = Vec<KnownIssueData>),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn list_known_issues(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<VersionIdArgs>,
) -> Result<Json<Vec<KnownIssueData>>> {
	let mut conn = state.db.get().await?;
	let v = Version::get_by_id(&mut conn, args.version_id).await?;
	let rows = VersionKnownIssue::list_for_minor(&mut conn, v.major, v.minor).await?;
	Ok(Json(rows.into_iter().map(KnownIssueData::from).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct AddKnownIssueArgs {
	pub version_id: Uuid,
	pub description: String,
}

#[utoipa::path(
	post,
	path = "/add_known_issue",
	tag = "versions",
	security(("tailscale-admin" = [])),
	request_body = AddKnownIssueArgs,
	responses(
		(status = 200, body = KnownIssueData),
		(status = 400, body = ProblemDetailsSchema),
	),
)]
pub async fn add_known_issue(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<AddKnownIssueArgs>,
) -> Result<Json<KnownIssueData>> {
	let description = args.description.trim();
	if description.is_empty() {
		return Err(AppError::BadRequest("Description must not be empty".into()));
	}
	let mut conn = state.db.get().await?;
	let v = Version::get_by_id(&mut conn, args.version_id).await?;
	let row = VersionKnownIssue::add(
		&mut conn,
		(v.major, v.minor, v.patch),
		&admin.0.login,
		description,
	)
	.await?;
	Ok(Json(KnownIssueData::from(row)))
}

#[derive(Deserialize, ToSchema)]
pub struct ResolveKnownIssueArgs {
	pub known_issue_id: Uuid,
	/// Semver of the version that contains the fix. Must be in the same
	/// minor as the issue's `min` and strictly above it.
	pub fix_version: String,
	pub resolution_message: String,
}

#[utoipa::path(
	post,
	path = "/resolve_known_issue",
	tag = "versions",
	security(("tailscale-admin" = [])),
	request_body = ResolveKnownIssueArgs,
	responses(
		(status = 200, body = KnownIssueData),
		(status = 400, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn resolve_known_issue(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<ResolveKnownIssueArgs>,
) -> Result<Json<KnownIssueData>> {
	let resolution = args.resolution_message.trim();
	if resolution.is_empty() {
		return Err(AppError::BadRequest(
			"Resolution message must not be empty".into(),
		));
	}
	let fix = VersionStr::from_str(&args.fix_version)?;
	let fix = (fix.0.major as i32, fix.0.minor as i32, fix.0.patch as i32);
	let mut conn = state.db.get().await?;
	let row = VersionKnownIssue::resolve(
		&mut conn,
		args.known_issue_id,
		fix,
		&admin.0.login,
		resolution,
	)
	.await?;
	Ok(Json(KnownIssueData::from(row)))
}
