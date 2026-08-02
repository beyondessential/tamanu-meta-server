use uuid::Uuid;

/// `get_snippet` declares 404 for a snippet that isn't there, but built the
/// miss with `AppError::custom`, which maps to 500 `/errors/other`. A client
/// following the checked-in contract saw a server fault where it should have
/// seen an ordinary miss.
#[tokio::test(flavor = "multi_thread")]
async fn get_snippet_is_404_for_an_unknown_id() {
	commons_tests::server::run(async |_conn, _public, private| {
		let resp = private
			.post("/api/bestool/get_snippet")
			.json(&serde_json::json!({ "id": Uuid::new_v4() }))
			.await;

		assert_eq!(resp.status_code().as_u16(), 404, "body: {}", resp.text());
		let body: serde_json::Value = resp.json();
		assert!(
			body["type"]
				.as_str()
				.is_some_and(|t| t.ends_with("resource-not-found")),
			"a miss should carry the resource-not-found problem type, got {body}",
		);
	})
	.await
}
