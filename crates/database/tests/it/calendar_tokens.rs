//! Lifecycle of calendar feed tokens: mint hands out a prefixed plaintext and
//! persists only the hash; find_active refuses revoked tokens; revoke is
//! idempotent but 404s on unknown ids.

use database::calendar_tokens::{CalendarToken, TOKEN_PREFIX};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread")]
async fn mint_find_touch_revoke_lifecycle() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let (token, plaintext) = CalendarToken::mint(&mut conn, "ops team", "admin@example.com")
			.await
			.expect("mint");
		assert!(plaintext.starts_with(TOKEN_PREFIX));
		assert_eq!(token.name, "ops team");
		assert_eq!(token.created_by, "admin@example.com");
		assert!(token.revoked_at.is_none());
		assert!(token.last_used_at.is_none());

		let found = CalendarToken::find_active(&mut conn, &plaintext)
			.await
			.expect("find")
			.expect("token is active");
		assert_eq!(found.id, token.id);

		assert!(
			CalendarToken::find_active(&mut conn, "canopy_cal_not-a-real-token")
				.await
				.expect("find")
				.is_none()
		);

		CalendarToken::touch_last_used(&mut conn, token.id)
			.await
			.expect("touch");
		let listed = CalendarToken::list(&mut conn).await.expect("list");
		assert_eq!(listed.len(), 1);
		assert!(listed[0].last_used_at.is_some());

		CalendarToken::revoke(&mut conn, token.id)
			.await
			.expect("revoke");
		assert!(
			CalendarToken::find_active(&mut conn, &plaintext)
				.await
				.expect("find")
				.is_none(),
			"a revoked feed serves nothing"
		);

		CalendarToken::revoke(&mut conn, token.id)
			.await
			.expect("revoking twice is fine");
		assert!(
			CalendarToken::revoke(&mut conn, Uuid::new_v4())
				.await
				.is_err(),
			"an unknown id is not found"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_plaintext_is_never_stored() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let (token, plaintext) = CalendarToken::mint(&mut conn, "ops team", "admin@example.com")
			.await
			.expect("mint");
		assert!(!token.token_hash.is_empty());
		assert!(
			!String::from_utf8_lossy(&token.token_hash).contains(&plaintext),
			"only the digest is persisted"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn dormancy_sweep_files_one_self_alert_and_recovers() {
	use database::calendar_tokens::{DORMANT_REF, sweep_dormant_feeds};
	use database::self_alerts;
	use diesel_async::SimpleAsyncConnection as _;

	commons_tests::db::TestDb::run(async |mut conn, _url| {
		// A fresh feed nobody has fetched yet is not dormant: it has not had
		// the window to be read in.
		let (token, _) = CalendarToken::mint(&mut conn, "ops team", "admin@example.com")
			.await
			.expect("mint");
		assert_eq!(sweep_dormant_feeds(&mut conn).await.expect("sweep"), 0);
		assert!(
			self_alerts::list(&mut conn, 50)
				.await
				.expect("list")
				.is_empty(),
			"idle sweep must not file alerts"
		);

		// Minted long enough ago with nothing ever reading it: dormant.
		conn.batch_execute(&format!(
			"UPDATE calendar_tokens SET created_at = NOW() - INTERVAL '90 days' WHERE id = '{}'",
			token.id
		))
		.await
		.expect("age feed");

		assert_eq!(sweep_dormant_feeds(&mut conn).await.expect("sweep"), 1);
		let alerts = self_alerts::list(&mut conn, 50).await.expect("list");
		let [issue] = alerts
			.iter()
			.filter(|i| i.r#ref == DORMANT_REF)
			.collect::<Vec<_>>()[..]
		else {
			panic!("exactly one self-alert, got: {alerts:?}");
		};
		assert!(issue.active);
		assert_eq!(issue.server_id, None);
		assert_eq!(issue.server_group_id, None);
		assert!(issue.message.contains("ops team"), "{}", issue.message);
		assert!(issue.message.contains("never fetched"), "{}", issue.message);

		// A calendar reading it again recovers the alert.
		CalendarToken::touch_last_used(&mut conn, token.id)
			.await
			.expect("touch");
		assert_eq!(sweep_dormant_feeds(&mut conn).await.expect("sweep"), 1);
		assert!(
			self_alerts::list(&mut conn, 50)
				.await
				.expect("list")
				.iter()
				.filter(|i| i.r#ref == DORMANT_REF)
				.all(|i| !i.active),
			"alert must recover once the feed is read again"
		);
		assert_eq!(sweep_dormant_feeds(&mut conn).await.expect("sweep"), 0);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_feed_does_not_alert() {
	use database::calendar_tokens::sweep_dormant_feeds;
	use diesel_async::SimpleAsyncConnection as _;

	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let (token, _) = CalendarToken::mint(&mut conn, "ops team", "admin@example.com")
			.await
			.expect("mint");
		conn.batch_execute(&format!(
			"UPDATE calendar_tokens SET created_at = NOW() - INTERVAL '90 days' WHERE id = '{}'",
			token.id
		))
		.await
		.expect("age feed");
		CalendarToken::revoke(&mut conn, token.id)
			.await
			.expect("revoke");

		assert_eq!(
			sweep_dormant_feeds(&mut conn).await.expect("sweep"),
			0,
			"a revoked feed is already dealt with"
		);
	})
	.await
}
