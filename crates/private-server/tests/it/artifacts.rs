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

		// The private detail page calls the same deduplicated view that the
		// public API serves: among multiple ranges that match a version, only
		// the most specific one wins.
		let response = private
			.post("/api/versions/get_version_artifacts")
			.json(&serde_json::json!({"version": "2.44.5"}))
			.await;

		response.assert_status_ok();
		let artifacts: Vec<ArtifactData> = response.json();

		assert_eq!(
			artifacts.len(),
			1,
			"deduplicated view should keep only the more specific range"
		);
		let chosen = &artifacts[0];
		assert_eq!(chosen.id.to_string(), narrower_range_id.to_lowercase());
		assert_eq!(
			chosen.version_range_pattern,
			Some("^2.44.2".to_string()),
			"the more specific range should be the one returned"
		);
		assert!(chosen.is_used_in_public_api);
	})
	.await
}
