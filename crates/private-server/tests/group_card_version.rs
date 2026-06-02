//! The status group card's headline version must come from the highest-ranked,
//! highest-kind member (e.g. the production central), not whichever member
//! reported most recently.

use commons_tests::diesel_async::SimpleAsyncConnection;
use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread")]
async fn group_card_version_is_from_highest_rank_kind_member() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status)
				VALUES (2, 10, 0, 'x', 'published');
			INSERT INTO server_groups (id, name)
				VALUES ('aaaaaaaa-0000-0000-0000-000000000001', 'Group');
			INSERT INTO servers (id, name, host, kind, rank, group_id) VALUES
				('aaaaaaaa-0000-0000-0000-0000000000c0', 'prod-central',
				 'https://prod.example.com', 'central', 'production',
				 'aaaaaaaa-0000-0000-0000-000000000001'),
				('aaaaaaaa-0000-0000-0000-0000000000d0', 'dev-facility',
				 'https://dev.example.com', 'facility', 'dev',
				 'aaaaaaaa-0000-0000-0000-000000000001');
			-- The dev/facility server reported MORE recently, with a different
			-- version; the prod/central reported earlier. The card must show the
			-- prod/central version regardless of recency.
			INSERT INTO statuses (server_id, created_at, version, healthy, health) VALUES
				('aaaaaaaa-0000-0000-0000-0000000000c0', NOW() - INTERVAL '1 hour',
				 '2.10.0', true, '[]'::jsonb),
				('aaaaaaaa-0000-0000-0000-0000000000d0', NOW() - INTERVAL '1 minute',
				 '2.99.0', true, '[]'::jsonb);",
		)
		.await
		.unwrap();

		let resp = private
			.post("/api/statuses/group_details")
			.json(&json!({ "server_group_id": "aaaaaaaa-0000-0000-0000-000000000001" }))
			.await;
		resp.assert_status_ok();
		let card: Value = resp.json();
		assert_eq!(
			card["version"], "2.10.0",
			"card shows the production-central version, not the most-recent (dev) one",
		);
	})
	.await;
}
