//! Snippet authorship. A snippet is attributed to the Tailscale user who
//! saved it, and that attribution is the whole audit trail for the SQL the
//! snippet carries.
//!
//! Note on coverage: the extractor short-circuits to `admin@localhost` under
//! `cfg!(debug_assertions)`, so these tests cannot exercise the
//! missing-identity path — that only exists in release builds. They pin the
//! observable contract (the editor is the caller, and is never blank); the
//! release-only behaviour is enforced by the extractor's type, not by a test.

use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_snippet_is_attributed_to_its_author() {
	commons_tests::server::run(async |_conn, _public, private| {
		let resp = private
			.post("/api/bestool/save_snippet")
			.json(&json!({
				"name": "daily-counts",
				"description": "row counts by table",
				"sql": "SELECT 1",
			}))
			.await;
		resp.assert_status_ok();
		let snippet: Value = resp.json();

		let editor = snippet["editor"].as_str().expect("editor is a string");
		assert!(
			!editor.is_empty(),
			"a snippet with a blank author has no audit trail",
		);
		assert_eq!(editor, "admin@localhost", "attributed to the caller");
	})
	.await
}
