use commons_tests::diesel_async::SimpleAsyncConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactData {
	pub id: Uuid,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
	pub is_exact: bool,
	pub version_range_pattern: Option<String>,
	pub has_range_override: bool,
	pub is_used_in_public_api: bool,
}

#[tokio::test(flavor = "multi_thread")]
async fn artifact_multiple_ranges_pattern_specificity_private_endpoint() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let version_id_245 = "44444444-4444-4444-4444-444444444444";
		let broader_range_id = "55555555-5555-5555-5555-555555555555";
		let narrower_range_id = "66666666-6666-6666-6666-666666666666";

		conn.batch_execute(&format!(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
			('{version_id_245}', 2, 44, 5, 'v2.44.5', 'published');

			INSERT INTO artifacts (id, version_id, platform, artifact_type, download_url, version_range_pattern) VALUES
			('{broader_range_id}', NULL, 'windows', 'installer', 'https://example.com/2.44.x.exe', '2.44.x'),
			('{narrower_range_id}', NULL, 'windows', 'installer', 'https://example.com/caret.exe', '^2.44.2')",
		))
		.await
		.unwrap();

		// Call the private server endpoint for getting artifacts by version ID
		// The private server should show ALL matching artifacts, not just the deduplicated public view
		let response = private
			.post("/api/private_server/fns/versions/get_artifacts_by_version_id")
			.form(&[("version_id", version_id_245)])
			.await;

		response.assert_status_ok();
		let artifacts: Vec<ArtifactData> = response.json();

		// Should have 2 artifacts - both ranges apply to 2.44.5
		assert_eq!(artifacts.len(), 2, "Should have 2 matching artifacts in private view");

		// Find each artifact
		let broader = artifacts.iter().find(|a| a.id.to_string() == broader_range_id.to_lowercase());
		let narrower = artifacts.iter().find(|a| a.id.to_string() == narrower_range_id.to_lowercase());

		assert!(broader.is_some(), "Should have the 2.44.x range artifact");
		assert!(narrower.is_some(), "Should have the ^2.44.2 range artifact");

		// Verify both are range artifacts (not exact)
		assert!(!broader.unwrap().is_exact, "2.44.x should be a ranged artifact");
		assert!(!narrower.unwrap().is_exact, "^2.44.2 should be a ranged artifact");

		// Verify they have the correct patterns
		assert_eq!(
			broader.unwrap().version_range_pattern,
			Some("2.44.x".to_string()),
			"Should have 2.44.x pattern"
		);
		assert_eq!(
			narrower.unwrap().version_range_pattern,
			Some("^2.44.2".to_string()),
			"Should have ^2.44.2 pattern"
		);

		// Verify which one is used in the public API
		// The more specific range (^2.44.2) should be used in the public API
		assert!(
			narrower.unwrap().is_used_in_public_api,
			"The more specific range (^2.44.2) should be used in the public API"
		);
		assert!(
			!broader.unwrap().is_used_in_public_api,
			"The less specific range (2.44.x) should NOT be used in the public API"
		);
	})
	.await
}
