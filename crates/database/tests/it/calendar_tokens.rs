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
