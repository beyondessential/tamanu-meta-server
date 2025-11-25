use commons_errors::Result;
use commons_types::version::VersionStatus;
use leptos::server;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionData {
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub status: VersionStatus,
	pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinorVersionGroup {
	pub major: i32,
	pub minor: i32,
	pub count: usize,
	pub latest_patch: i32,
	pub first_created_at: String,
	pub last_created_at: String,
	pub versions: Vec<VersionData>,
}

#[server]
pub async fn get_grouped_versions() -> Result<Vec<MinorVersionGroup>> {
	ssr::get_grouped_versions().await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use std::collections::BTreeMap;

	use axum::extract::State;
	use commons_errors::Result;
	use database::{Db, versions::Version};
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
				.or_insert_with(Vec::new)
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
							.unwrap_or_else(|| chrono::Utc::now())
					});

				let last_created_at = published_versions
					.first()
					.map(|v| v.created_at)
					.unwrap_or_else(|| chrono::Utc::now());

				let version_data: Vec<VersionData> = versions
					.into_iter()
					.map(|v| VersionData {
						major: v.major,
						minor: v.minor,
						patch: v.patch,
						status: v.status,
						created_at: v.created_at.format("%Y-%m-%d").to_string(),
					})
					.collect();

				MinorVersionGroup {
					major,
					minor,
					count,
					latest_patch,
					first_created_at: first_created_at.format("%Y-%m-%d").to_string(),
					last_created_at: last_created_at.format("%Y-%m-%d").to_string(),
					versions: version_data,
				}
			})
			.collect();

		result.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| b.minor.cmp(&a.minor)));

		Ok(result)
	}
}
