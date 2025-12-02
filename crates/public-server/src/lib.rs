#[cfg(feature = "ui")]
use axum::extract::State;
use axum::routing::Router;

use crate::state::AppState;

pub mod artifacts;
#[cfg(feature = "ui")]
pub mod password;
#[cfg(feature = "ui")]
pub mod server_versions;
pub mod servers;
pub mod state;
pub mod statuses;
#[cfg(feature = "ui")]
pub mod timesync;
pub mod versions;

pub fn routes() -> Router<AppState> {
	#[cfg_attr(not(feature = "ui"), expect(unused_mut))]
	let mut router = Router::new()
		.nest("/artifacts", artifacts::routes())
		.nest("/servers", servers::routes())
		.nest("/status", statuses::routes())
		.nest("/versions", versions::routes());

	#[cfg(feature = "ui")]
	{
		use axum::routing::get;
		use tower_http::services::ServeDir;
		router = router
			.route("/", get(index))
			.route("/errors/{slug}", get(error))
			.merge(commons_servers::health::routes())
			.merge(timesync::routes())
			.merge(password::routes())
			.nest_service("/static", ServeDir::new("static"));

		// Mount server-versions route (secret is checked in the handler)
		router = router.nest("/server-versions", server_versions::routes());
	}

	router
}

#[cfg(feature = "ui")]
async fn index(
	State(db): State<database::Db>,
	State(tera): State<std::sync::Arc<tera::Tera>>,
) -> commons_errors::Result<axum::response::Html<String>> {
	use commons_types::version::VersionStatus;
	use database::versions::Version;
	use serde::Serialize;
	use std::collections::BTreeMap;
	use tera::Context;

	#[derive(Debug, Clone, Serialize)]
	struct VersionData {
		major: i32,
		minor: i32,
		patch: i32,
		status: String,
		created_at: jiff::Timestamp,
	}

	#[derive(Debug, Clone, Serialize)]
	struct MinorVersionGroup {
		major: i32,
		minor: i32,
		count: usize,
		latest_patch: i32,
		first_created_at: jiff::Timestamp,
		versions: Vec<VersionData>,
	}

	let mut db = db.get().await?;
	let versions = Version::get_all_including_drafts(&mut db).await?;

	let mut grouped: BTreeMap<(i32, i32), Vec<Version>> = BTreeMap::new();
	for version in versions {
		grouped
			.entry((version.major, version.minor))
			.or_insert_with(Vec::new)
			.push(version);
	}

	let mut groups: Vec<MinorVersionGroup> = grouped
		.into_iter()
		.filter_map(|((major, minor), mut versions)| {
			// Filter to only published versions
			versions.retain(|v| v.status == VersionStatus::Published);

			// Skip groups with no published versions
			if versions.is_empty() {
				return None;
			}

			versions.sort_by(|a, b| b.patch.cmp(&a.patch));

			let count = versions.len();
			let latest_patch = versions.first().map(|v| v.patch).unwrap_or(0);

			let first_created_at = versions
				.iter()
				.find(|v| v.patch == 0)
				.map(|v| v.created_at)
				.unwrap_or_else(|| versions.last().map(|v| v.created_at).unwrap());

			let version_data: Vec<VersionData> = versions
				.into_iter()
				.map(|v| VersionData {
					major: v.major,
					minor: v.minor,
					patch: v.patch,
					status: v.status.to_string().to_lowercase(),
					created_at: v.created_at,
				})
				.collect();

			Some(MinorVersionGroup {
				major,
				minor,
				count,
				latest_patch,
				first_created_at,
				versions: version_data,
			})
		})
		.collect();

	groups.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| b.minor.cmp(&a.minor)));

	let env = std::env::vars().collect::<std::collections::BTreeMap<String, String>>();
	let mut context = Context::new();
	context.insert("groups", &groups);
	context.insert("env", &env);
	let html = tera.render("versions", &context)?;
	Ok(axum::response::Html(html))
}

#[cfg(feature = "ui")]
async fn error(axum::extract::Path(slug): axum::extract::Path<String>) -> axum::response::Redirect {
	axum::response::Redirect::temporary(&format!(
		"https://github.com/beyondessential/tamanu-meta-server/blob/{version}/ERRORS.md#{slug}",
		version = env!("CARGO_PKG_VERSION")
	))
}
