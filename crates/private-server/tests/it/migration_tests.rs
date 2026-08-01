//! The operator's view of where a group stands: one entry per server with a
//! candidate version, carrying its verdict and the test behind it.

use commons_tests::diesel_async::SimpleAsyncConnection;
use serde_json::{Value, json};

const GROUP: &str = "bbbbbbbb-0000-0000-0000-000000000001";
const BEHIND: &str = "bbbbbbbb-0000-0000-0000-0000000000a0";
const CURRENT: &str = "bbbbbbbb-0000-0000-0000-0000000000b0";

#[tokio::test(flavor = "multi_thread")]
async fn for_group_reports_a_verdict_per_candidate() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, status)
				VALUES (2, 62, 0, 'x', 'published'), (2, 63, 0, 'x', 'published');
			INSERT INTO server_groups (id, name)
				VALUES ('bbbbbbbb-0000-0000-0000-000000000001', 'Kamaka');
			INSERT INTO servers (id, name, host, kind, group_id) VALUES
				('bbbbbbbb-0000-0000-0000-0000000000a0', 'behind',
				 'https://behind.example.com', 'central',
				 'bbbbbbbb-0000-0000-0000-000000000001'),
				('bbbbbbbb-0000-0000-0000-0000000000b0', 'current',
				 'https://current.example.com', 'facility',
				 'bbbbbbbb-0000-0000-0000-000000000001');
			INSERT INTO server_reported_detail (server_id, source, extra, version) VALUES
				('bbbbbbbb-0000-0000-0000-0000000000a0', 'test', '{}'::jsonb, '2.62.0'),
				('bbbbbbbb-0000-0000-0000-0000000000b0', 'test', '{}'::jsonb, '2.63.0');",
		)
		.await
		.unwrap();

		let resp = private
			.post("/api/migration_tests/for_group")
			.json(&json!({ "group_id": GROUP }))
			.await;
		resp.assert_status_ok();
		let verdicts: Vec<Value> = resp.json();

		assert_eq!(
			verdicts.len(),
			1,
			"only the server with somewhere to upgrade to: got {verdicts:?}"
		);
		assert_eq!(verdicts[0]["server_id"], BEHIND);
		assert_eq!(verdicts[0]["target_version"], "2.63.0");
		assert_eq!(verdicts[0]["verdict"], "nottested");
		assert!(verdicts[0]["latest"].is_null());

		let current_absent = verdicts.iter().all(|v| v["server_id"] != CURRENT);
		assert!(
			current_absent,
			"a server already on the newest version has nothing to test"
		);
	})
	.await;
}
