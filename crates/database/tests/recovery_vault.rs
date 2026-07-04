//! Recovery vault write bookkeeping: `record` inserts one row per successful
//! vault write, `latest` reports the most recent one (or `None` before the
//! first write).

use database::RecoveryVaultWrite;

#[tokio::test(flavor = "multi_thread")]
async fn record_and_latest() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		assert!(
			RecoveryVaultWrite::latest(&mut conn)
				.await
				.expect("latest")
				.is_none(),
			"no writes yet"
		);

		let first = RecoveryVaultWrite::record(&mut conn, 1_000)
			.await
			.expect("record first");
		let latest = RecoveryVaultWrite::latest(&mut conn)
			.await
			.expect("latest")
			.expect("a write exists");
		assert_eq!(latest.id, first.id);
		assert_eq!(latest.bytes, 1_000);

		let second = RecoveryVaultWrite::record(&mut conn, 2_048)
			.await
			.expect("record second");
		let latest = RecoveryVaultWrite::latest(&mut conn)
			.await
			.expect("latest")
			.expect("a write exists");
		assert_eq!(latest.id, second.id, "latest is the most recent write");
		assert_eq!(latest.bytes, 2_048);
	})
	.await
}
