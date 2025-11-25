use commons_tests::{diesel_async::SimpleAsyncConnection, server};

#[tokio::test(flavor = "multi_thread")]
async fn test_get_grouped_versions() {
	server::run(|mut conn, _public, private| async move {
		// Create some test versions
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, status, changelog) VALUES
			('11111111-1111-1111-1111-111111111111', 1, 0, 0, 'published', 'Version 1.0.0'),
			('22222222-2222-2222-2222-222222222222', 1, 0, 1, 'published', 'Version 1.0.1'),
			('33333333-3333-3333-3333-333333333333', 1, 0, 2, 'published', 'Version 1.0.2'),
			('44444444-4444-4444-4444-444444444444', 1, 1, 0, 'published', 'Version 1.1.0'),
			('55555555-5555-5555-5555-555555555555', 1, 1, 1, 'published', 'Version 1.1.1'),
			('66666666-6666-6666-6666-666666666666', 2, 0, 0, 'published', 'Version 2.0.0'),
			('77777777-7777-7777-7777-777777777777', 2, 0, 1, 'published', 'Version 2.0.1'),
			('88888888-8888-8888-8888-888888888888', 2, 0, 2, 'draft', 'Version 2.0.2 draft'),
			('99999999-9999-9999-9999-999999999999', 2, 0, 3, 'yanked', 'Version 2.0.3 yanked')",
		)
		.await
		.unwrap();

		// Test the server function endpoint
		let response = private
			.post("/api/private_server/fns/versions/get_grouped_versions")
			.await;

		assert_eq!(response.status_code(), 200);

		let groups: Vec<serde_json::Value> = response.json();

		// Should have 3 groups: 2.0, 1.1, 1.0 (sorted by major.minor descending)
		assert_eq!(groups.len(), 3);

		// Check 2.0 group (includes draft and yanked, but they don't affect latest_patch)
		let group_2_0 = &groups[0];
		assert_eq!(group_2_0["major"], 2);
		assert_eq!(group_2_0["minor"], 0);
		assert_eq!(group_2_0["count"], 4); // Total includes draft and yanked
		assert_eq!(group_2_0["latest_patch"], 1); // Should be 1, not 3, because 2 is draft and 3 is yanked
		assert_eq!(group_2_0["versions"].as_array().unwrap().len(), 4);

		// Check that draft and yanked statuses are included in the list
		let versions_2_0 = group_2_0["versions"].as_array().unwrap();
		assert_eq!(versions_2_0[0]["patch"], 3);
		assert_eq!(versions_2_0[0]["status"], "yanked");
		assert_eq!(versions_2_0[1]["patch"], 2);
		assert_eq!(versions_2_0[1]["status"], "draft");
		assert_eq!(versions_2_0[2]["patch"], 1);
		assert_eq!(versions_2_0[2]["status"], "published");

		// Check 1.1 group
		let group_1_1 = &groups[1];
		assert_eq!(group_1_1["major"], 1);
		assert_eq!(group_1_1["minor"], 1);
		assert_eq!(group_1_1["count"], 2);
		assert_eq!(group_1_1["latest_patch"], 1);
		assert_eq!(group_1_1["versions"].as_array().unwrap().len(), 2);

		// Check 1.0 group
		let group_1_0 = &groups[2];
		assert_eq!(group_1_0["major"], 1);
		assert_eq!(group_1_0["minor"], 0);
		assert_eq!(group_1_0["count"], 3);
		assert_eq!(group_1_0["latest_patch"], 2);
		assert_eq!(group_1_0["versions"].as_array().unwrap().len(), 3);

		// Verify versions within 1.0 are sorted by patch descending
		let versions_1_0 = group_1_0["versions"].as_array().unwrap();
		assert_eq!(versions_1_0[0]["patch"], 2);
		assert_eq!(versions_1_0[1]["patch"], 1);
		assert_eq!(versions_1_0[2]["patch"], 0);
	})
	.await;
}
