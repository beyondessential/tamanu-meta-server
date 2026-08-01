//! The `version_updates` view: one row per `(major, minor)` line, and which
//! row that is when the newest patch in a line isn't published.

use commons_tests::db::TestDb;
use commons_types::version::VersionStr;
use database::versions::Version;
use diesel_async::SimpleAsyncConnection;

async fn updates_from(
	conn: &mut database::diesel_async::AsyncPgConnection,
	from: &str,
) -> Vec<(i32, i32, i32)> {
	Version::get_updates_for_version(conn, from.parse::<VersionStr>().expect("parse version"))
		.await
		.expect("updates")
		.into_iter()
		.map(|v| (v.major, v.minor, v.patch))
		.collect()
}

/// A line's newest patch being draft (or yanked) must not take the whole line
/// with it: the update on offer is the line's newest *published* patch. The
/// view reduces each line to one row, so filtering status after that reduction
/// is too late.
#[tokio::test(flavor = "multi_thread")]
async fn a_line_offers_its_newest_published_patch_not_nothing() {
	TestDb::run(|mut conn, _url| async move {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES
			(2, 45, 0, 'v2.45.0', 'published'),
			(2, 46, 1, 'v2.46.1', 'published'),
			(2, 46, 2, 'v2.46.2', 'published'),
			(2, 46, 3, 'v2.46.3', 'draft'),
			(2, 47, 0, 'v2.47.0', 'published'),
			(2, 47, 1, 'v2.47.1', 'yanked')",
		)
		.await
		.expect("seed versions");

		assert_eq!(
			updates_from(&mut conn, "2.45.0").await,
			vec![(2, 46, 2), (2, 47, 0)],
			"each line offers its newest published patch",
		);
	})
	.await;
}

/// A line with nothing published in it offers nothing, rather than leaking an
/// unpublished version as an available update.
#[tokio::test(flavor = "multi_thread")]
async fn a_wholly_unpublished_line_offers_nothing() {
	TestDb::run(|mut conn, _url| async move {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) VALUES
			(3, 0, 0, 'v3.0.0', 'published'),
			(3, 1, 0, 'v3.1.0', 'draft'),
			(3, 1, 1, 'v3.1.1', 'draft')",
		)
		.await
		.expect("seed versions");

		assert!(
			updates_from(&mut conn, "3.0.0").await.is_empty(),
			"a draft-only line is not an available update",
		);
	})
	.await;
}
