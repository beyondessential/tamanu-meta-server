//! The `valid_snippet_name` CHECK. Snippet names become client-side filenames
//! on Windows bestool, which is why the forbidden set is what it is.
//!
//! Backslash is LIKE's default escape character, so the clause meant to reject
//! backslashes read as "does not end in a percent sign" instead — accepting
//! exactly what it was there to stop, and rejecting something harmless.

use commons_tests::db::TestDb;
use database::bestool_snippets::BestoolSnippet;

async fn create(
	conn: &mut database::diesel_async::AsyncPgConnection,
	name: &str,
) -> commons_errors::Result<BestoolSnippet> {
	BestoolSnippet::create(
		conn,
		"op@bes".into(),
		name.into(),
		None,
		"SELECT 1".into(),
		None,
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backslash_in_a_name_is_rejected() {
	TestDb::run(|mut conn, _url| async move {
		assert!(
			create(&mut conn, r"report\daily").await.is_err(),
			"a backslash makes an unusable filename on Windows",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_percent_sign_in_a_name_is_allowed() {
	TestDb::run(|mut conn, _url| async move {
		create(&mut conn, "top100%")
			.await
			.expect("a percent sign was never on the forbidden list");
		create(&mut conn, "50%-done")
			.await
			.expect("nor is one in the middle");
	})
	.await;
}

/// The rest of the forbidden set is unaffected by the fix.
#[tokio::test(flavor = "multi_thread")]
async fn the_other_forbidden_names_are_still_rejected() {
	TestDb::run(|mut conn, _url| async move {
		for name in [
			"has space",
			"has.dot",
			"has/slash",
			"has<lt",
			"has>gt",
			"has:colon",
			"has\"quote",
			"has'apos",
			"has|pipe",
			"has?q",
			"has*star",
			"CON",
			"lpt9",
		] {
			assert!(
				create(&mut conn, name).await.is_err(),
				"{name:?} must still be rejected",
			);
		}
		create(&mut conn, "perfectly-fine_name1")
			.await
			.expect("an ordinary name is accepted");
	})
	.await;
}
