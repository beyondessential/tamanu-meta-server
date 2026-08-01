use database::chrome_releases::{ChromeRelease, NewChromeRelease};
use jiff::Timestamp;

async fn seed(conn: &mut diesel_async::AsyncPgConnection, version: &str, released: &str) {
	NewChromeRelease {
		version: version.to_string(),
		release_date: released.to_string(),
		is_eol: false,
		eol_from: None,
	}
	.save(conn)
	.await
	.expect("seed chrome release");
}

fn at(date: &str) -> Timestamp {
	format!("{date}T00:00:00Z").parse().expect("timestamp")
}

/// `version` is TEXT, so `ORDER BY version` is lexicographic: `"100"` sorts
/// before `"99"`. Whenever two majors of differing digit-length are live at
/// once — 99 and 100, and again at 999/1000 — the "oldest live major" came
/// back as the longer string rather than the smaller number.
#[tokio::test(flavor = "multi_thread")]
async fn min_version_is_the_smallest_number_not_the_smallest_string() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		seed(&mut conn, "99", "2026-01-01").await;
		seed(&mut conn, "100", "2026-02-01").await;
		seed(&mut conn, "101", "2026-03-01").await;

		let min = ChromeRelease::get_min_version_at_date(&mut conn, at("2026-04-01"))
			.await
			.expect("min version");

		// The reported minimum is one below the oldest live major.
		assert_eq!(min, Some(98), "99 is the oldest live major, not 100");
	})
	.await
}

/// The same ordering backs the catalog listing.
#[tokio::test(flavor = "multi_thread")]
async fn get_all_is_ordered_numerically() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		for (v, d) in [
			("100", "2026-02-01"),
			("9", "2025-11-01"),
			("99", "2026-01-01"),
			("1000", "2026-05-01"),
		] {
			seed(&mut conn, v, d).await;
		}

		let all = ChromeRelease::get_all(&mut conn).await.expect("get all");
		let versions: Vec<&str> = all.iter().map(|r| r.version.as_str()).collect();

		assert_eq!(versions, vec!["9", "99", "100", "1000"]);
	})
	.await
}

/// A row whose `version` isn't a plain number can't be read back as one
/// (`parse::<u32>()` drops it), so it must not be able to win the minimum
/// either — it sorts last rather than poisoning the query with a cast error.
#[tokio::test(flavor = "multi_thread")]
async fn a_non_numeric_version_does_not_break_or_win_the_ordering() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		seed(&mut conn, "not-a-version", "2025-01-01").await;
		seed(&mut conn, "120", "2026-01-01").await;

		let min = ChromeRelease::get_min_version_at_date(&mut conn, at("2026-04-01"))
			.await
			.expect("min version must not error on a non-numeric row");
		assert_eq!(min, Some(119));

		let all = ChromeRelease::get_all(&mut conn).await.expect("get all");
		assert_eq!(
			all.last().map(|r| r.version.as_str()),
			Some("not-a-version"),
		);
	})
	.await
}
