//! DB-layer tests for semver range resolution (`database::versions`).

use commons_tests::db::TestDb;
use database::versions::Version;
use diesel_async::SimpleAsyncConnection;

async fn seed(conn: &mut database::diesel_async::AsyncPgConnection, versions: &[(i32, i32, i32)]) {
	let values = versions
		.iter()
		.map(|(major, minor, patch)| {
			format!("({major}, {minor}, {patch}, 'v{major}.{minor}.{patch}', 'published')")
		})
		.collect::<Vec<_>>()
		.join(", ");
	conn.batch_execute(&format!(
		"INSERT INTO versions (major, minor, patch, changelog, status) VALUES {values}"
	))
	.await
	.expect("seed versions");
}

async fn latest_matching(
	conn: &mut database::diesel_async::AsyncPgConnection,
	range: &str,
) -> (i32, i32, i32) {
	let range: node_semver::Range = range.parse().expect("parse range");
	let v = Version::get_latest_matching(conn, range.clone())
		.await
		.unwrap_or_else(|e| panic!("no version matched {range}: {e}"));
	(v.major, v.minor, v.patch)
}

/// Semver orders versions lexicographically, not component-wise: the SQL
/// prefilter ahead of `Range::satisfies` must not drop a candidate whose
/// minor or patch is individually below the range's floor.
#[tokio::test(flavor = "multi_thread")]
async fn latest_matching_spans_minor_and_major_lines() {
	TestDb::run(|mut conn, _url| async move {
		seed(&mut conn, &[(1, 0, 5), (1, 1, 0), (2, 0, 0)]).await;

		// 1.1.0's patch (0) is below the floor's (5), and 2.0.0's minor is below
		// the floor's — both still satisfy the range.
		assert_eq!(latest_matching(&mut conn, ">=1.0.5").await, (2, 0, 0));

		// Caret ranges are capped at the major, so 1.1.0 wins over 1.0.5 despite
		// the lower patch.
		assert_eq!(latest_matching(&mut conn, "^1.0.5").await, (1, 1, 0));

		// The floor itself still resolves when nothing above it exists.
		assert_eq!(latest_matching(&mut conn, "~1.0.5").await, (1, 0, 5));
	})
	.await;
}

/// The cross-minor case that used to 404: the only candidate is above the
/// floor lexicographically but below it on the patch component.
#[tokio::test(flavor = "multi_thread")]
async fn latest_matching_finds_a_higher_minor_as_the_sole_candidate() {
	TestDb::run(|mut conn, _url| async move {
		seed(&mut conn, &[(2, 6, 0)]).await;
		assert_eq!(latest_matching(&mut conn, ">=2.5.1").await, (2, 6, 0));
	})
	.await;
}

/// `is_latest_in_minor` folded any Diesel error into `None` and then reported
/// `None` as "yes, latest" — the permissive answer, and the one guarding the
/// publish-to-draft demotion. These pin the answers the guard actually
/// depends on, so the swap from `.ok()` to `.optional()?` is behaviour-
/// preserving everywhere except the error path.
#[tokio::test(flavor = "multi_thread")]
async fn is_latest_in_minor_answers_within_the_minor_line() {
	TestDb::run(|mut conn, _url| async move {
		seed(&mut conn, &[(3, 4, 0), (3, 4, 1), (3, 5, 0)]).await;

		assert!(
			Version::is_latest_in_minor(&mut conn, "3.4.1".parse().unwrap())
				.await
				.expect("query"),
			"3.4.1 is the highest published patch in 3.4",
		);
		assert!(
			!Version::is_latest_in_minor(&mut conn, "3.4.0".parse().unwrap())
				.await
				.expect("query"),
			"3.4.0 is behind 3.4.1",
		);
		assert!(
			Version::is_latest_in_minor(&mut conn, "3.5.0".parse().unwrap())
				.await
				.expect("query"),
			"a later minor line is judged on its own",
		);
	})
	.await
}

/// A draft version has no published sibling to lose to, so it still reads as
/// latest — the genuine empty-result case, which must stay distinct from the
/// error case.
#[tokio::test(flavor = "multi_thread")]
async fn a_minor_line_with_nothing_published_reads_as_latest() {
	TestDb::run(|mut conn, _url| async move {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status) \
			 VALUES (4, 0, 0, 'v4.0.0', 'draft')",
		)
		.await
		.expect("seed draft");

		assert!(
			Version::is_latest_in_minor(&mut conn, "4.0.0".parse().unwrap())
				.await
				.expect("query"),
			"nothing published in 4.0 to be behind",
		);
	})
	.await
}
