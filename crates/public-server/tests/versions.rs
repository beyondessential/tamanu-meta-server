use axum::http::StatusCode;
use commons_types::version::VersionStatus;
use diesel_async::SimpleAsyncConnection;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Version {
	pub id: Uuid,
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub changelog: String,
	pub status: VersionStatus,
	pub device_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Artifact {
	pub id: Uuid,
	pub version_id: Uuid,
	pub platform: String,
	pub artifact_type: String,
	pub download_url: String,
	pub device_id: Option<Uuid>,
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_versions_list() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/versions").await;
		response.assert_status_ok();
		response.assert_json::<Vec<Version>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn versions_list_with_data() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES (1, 0, 0, 'Initial release', 'published')",
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
		assert_eq!(versions[0].status, VersionStatus::Published);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn view_version_artifacts_html() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES (1, 0, 0, 'Test version', 'published')",
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
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES (1, 0, 0, 'Test version', 'published')",
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
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES ('11111111-1111-1111-1111-111111111111', 1, 0, 0, 'Test version', 'published');
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
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES (1, 0, 0, 'Test version', 'published')",
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
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES (1, 0, 0, 'Test version', 'published')",
		)
		.await
		.unwrap();

		let response = public.get("/versions/update-for/1.0.0").await;
		println!("{:?}", response.as_bytes());
		response.assert_status_ok();
		response.assert_json::<Vec<Version>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_for_version_with_newer() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES
			(1, 0, 0, 'Old version', 'published'),
			(1, 0, 1, 'Patch update', 'published'),
			(1, 1, 0, 'Minor update', 'published')",
		)
		.await
		.unwrap();

		let response = public.get("/versions/update-for/1.0.0").await;
		println!("{:?}", response.as_bytes());
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
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/versions/999.999.999").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_not_found_artifacts() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/versions/999.999.999/artifacts").await;
		response.assert_status(StatusCode::NOT_FOUND);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn artifacts_create_draft_version_if_not_found() {
	use database::versions::Version;

	commons_tests::server::run_with_device_auth(
		"releaser",
		async |mut conn, cert, device_id, public, _| {
			let response = public
				.post("/artifacts/999.999.999/installer/windows")
				.add_header("mtls-certificate", &cert)
				.text("https://example.com/installer.exe")
				.await;
			response.assert_status_ok();

			let artifact: Artifact = response.json();
			assert_eq!(artifact.platform, "windows");
			assert_eq!(artifact.artifact_type, "installer");
			assert_eq!(artifact.download_url, "https://example.com/installer.exe");
			assert_eq!(artifact.device_id, Some(device_id));

			// Verify the version was created as a draft
			let version = Version::get_by_version(&mut conn, "999.999.999".parse().unwrap())
				.await
				.unwrap();
			assert_eq!(version.status, VersionStatus::Draft);
			assert_eq!(version.major, 999);
			assert_eq!(version.minor, 999);
			assert_eq!(version.patch, 999);
			assert_eq!(version.changelog, "");
			assert_eq!(version.device_id, Some(device_id));
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_create_publishes_draft_if_exists() {
	use database::versions::Version;

	commons_tests::server::run_with_device_auth(
		"releaser",
		async |mut conn, cert, device_id, public, _| {
			// First, create a draft version by creating an artifact
			let response = public
				.post("/artifacts/2.0.0/installer/windows")
				.add_header("mtls-certificate", &cert)
				.text("https://example.com/installer.exe")
				.await;
			response.assert_status_ok();

			// Verify it's a draft
			let version = Version::get_by_version(&mut conn, "2.0.0".parse().unwrap())
				.await
				.unwrap();
			assert_eq!(version.status, VersionStatus::Draft);
			assert_eq!(version.changelog, "");
			assert_eq!(version.device_id, Some(device_id));

			// Now create/publish the version with a changelog
			let changelog = "# Version 2.0.0\n\nNew features and improvements";
			let response = public
				.post("/versions/2.0.0")
				.add_header("mtls-certificate", &cert)
				.text(changelog)
				.await;
			response.assert_status_ok();

			let published_version: Version = response.json();
			assert_eq!(published_version.status, VersionStatus::Published);
			assert_eq!(published_version.changelog, changelog);
			assert_eq!(published_version.major, 2);
			assert_eq!(published_version.minor, 0);
			assert_eq!(published_version.patch, 0);
			assert_eq!(published_version.device_id, Some(device_id));

			// Verify the version in the database is now published with the new changelog and device_id
			let db_version = Version::get_by_version(&mut conn, "2.0.0".parse().unwrap())
				.await
				.unwrap();
			assert_eq!(db_version.status, VersionStatus::Published);
			assert_eq!(db_version.changelog, changelog);
			assert_eq!(db_version.device_id, Some(device_id));
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_create_duplicate_published_fails() {
	commons_tests::server::run_with_device_auth(
		"releaser",
		async |mut conn, cert, _device_id, public, _| {
			// Create a published version
			conn.batch_execute(
				"INSERT INTO versions (major, minor, patch, changelog, status) VALUES (3, 0, 0, 'First changelog', 'published')",
			)
			.await
			.unwrap();

			// Try to create the same version again - should fail
			let changelog = "# Version 3.0.0\n\nDifferent changelog";
			let response = public
				.post("/versions/3.0.0")
				.add_header("mtls-certificate", &cert)
				.text(changelog)
				.await;

			// Should fail with a database constraint error (500 or 409)
			assert!(
				response.status_code() == StatusCode::INTERNAL_SERVER_ERROR
				|| response.status_code() == StatusCode::CONFLICT,
				"Expected 500 or 409, got {}",
				response.status_code()
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn version_range_latest_matching() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES
			(1, 0, 0, 'Version 1.0.0', 'published'),
			(1, 0, 1, 'Version 1.0.1', 'published'),
			(1, 0, 2, 'Version 1.0.2', 'published')",
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
