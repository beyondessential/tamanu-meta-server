use axum::http::StatusCode;
use diesel_async::SimpleAsyncConnection;
use serde::Deserialize;
use uuid::Uuid;

#[path = "common/server.rs"]
mod test_server;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Version {
	pub id: Uuid,
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub changelog: String,
	pub published: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Artifact {
	pub id: Uuid,
	pub version_id: Uuid,
	pub platform: String,
	pub artifact_type: String,
	pub download_url: String,
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_versions_list() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/versions").await;
		response.assert_status_ok();
		response.assert_json::<Vec<Version>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn versions_list_with_data() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Initial release', true)",
		)
		.await
		.unwrap();

		let response = public.get("/versions").await;
		response.assert_status_ok();
		let versions: Vec<Version> = response.json();
		assert_eq!(versions.len(), 1);
		assert_eq!(versions[0].major, 1);
		assert_eq!(versions[0].minor, 0);
		assert_eq!(versions[0].patch, 0);
		assert_eq!(versions[0].changelog, "Initial release");
		assert!(versions[0].published);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn view_version_artifacts_html() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		let response = public.get("/versions/1.0.0").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn list_version_artifacts_empty() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		let response = public.get("/versions/1.0.0/artifacts").await;
		response.assert_status_ok();
		response.assert_json::<Vec<Artifact>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn list_version_artifacts_with_data() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, published) VALUES ('11111111-1111-1111-1111-111111111111', 1, 0, 0, 'Test version', true);
			INSERT INTO artifacts (version_id, platform, artifact_type, download_url) VALUES
			('11111111-1111-1111-1111-111111111111', 'windows', 'installer', 'https://example.com/installer.exe'),
			('11111111-1111-1111-1111-111111111111', 'macos', 'dmg', 'https://example.com/installer.dmg')",
		)
		.await
		.unwrap();

		let response = public.get("/versions/1.0.0/artifacts").await;
		response.assert_status_ok();
		let artifacts: Vec<Artifact> = response.json();
		assert_eq!(artifacts.len(), 2);

		let windows_artifact = artifacts.iter().find(|a| a.platform == "windows").unwrap();
		assert_eq!(windows_artifact.artifact_type, "installer");
		assert_eq!(windows_artifact.download_url, "https://example.com/installer.exe");

		let macos_artifact = artifacts.iter().find(|a| a.platform == "macos").unwrap();
		assert_eq!(macos_artifact.artifact_type, "dmg");
		assert_eq!(macos_artifact.download_url, "https://example.com/installer.dmg");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn view_mobile_install_page() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		let response = public.get("/versions/1.0.0/mobile").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_for_version_empty() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		let response = public.get("/versions/update-for/1.0.0").await;
		response.assert_status_ok();
		response.assert_json::<Vec<Version>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_for_version_with_newer() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES
			(1, 0, 0, 'Old version', true),
			(1, 0, 1, 'Patch update', true),
			(1, 1, 0, 'Minor update', true)",
		)
		.await
		.unwrap();

		let response = public.get("/versions/update-for/1.0.0").await;
		response.assert_status_ok();
		let updates: Vec<Version> = response.json();
		assert_eq!(updates.len(), 2);

		// Should include newer versions
		assert!(updates.iter().any(|v| v.patch == 1));
		assert!(updates.iter().any(|v| v.minor == 1));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_not_found() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/versions/999.999.999").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_range_latest_matching() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES
			(1, 0, 0, 'Version 1.0.0', true),
			(1, 0, 1, 'Version 1.0.1', true),
			(1, 0, 2, 'Version 1.0.2', true)",
		)
		.await
		.unwrap();

		// Should return latest patch version in 1.0.x range
		let response = public.get("/versions/1.0/artifacts").await;
		response.assert_status_ok();

		// Should also work with mobile page
		let response = public.get("/versions/1.0/mobile").await;
		response.assert_status_ok();
		response.assert_header("content-type", "text/html; charset=utf-8");
	})
	.await
}
