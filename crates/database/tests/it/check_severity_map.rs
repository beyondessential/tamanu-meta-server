//! Queries backing the device-facing effective check map:
//! `CheckPolicy::ceiling_map_for_source` (static policy ceilings,
//! ignoring conditional rules) and
//! `silenced_refs::silenced_health_checks_for_server` (server- plus
//! group-scope silences under one reporting source).

use commons_types::status::CheckResult;
use database::check_policies::{CheckPolicy, IfLadder};
use database::silenced_refs::{
	ServerGroupSilencedRef, ServerSilencedRef, silenced_health_checks_for_server,
};
use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use serde_json::json;
use uuid::Uuid;

async fn insert_group(conn: &mut diesel_async::AsyncPgConnection) -> Uuid {
	let group_id = Uuid::new_v4();
	sql_query("INSERT INTO server_groups (id, name) VALUES ($1, 'severity-map-group')")
		.bind::<sql_types::Uuid, _>(group_id)
		.execute(conn)
		.await
		.expect("insert group");
	group_id
}

async fn insert_server(conn: &mut diesel_async::AsyncPgConnection, group_id: Option<Uuid>) -> Uuid {
	let server_id = Uuid::new_v4();
	sql_query(
		"INSERT INTO servers (id, host, kind, group_id) \
		 VALUES ($1, 'https://severity-map.example.com', 'facility', $2)",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group_id)
	.execute(conn)
	.await
	.expect("insert server");
	server_id
}

#[tokio::test(flavor = "multi_thread")]
async fn ceiling_map_returns_static_ceilings_for_one_source() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		for check in ["disk_space", "cert_expiry", "chatty"] {
			CheckPolicy::upsert_default(&mut conn, "alertd", check)
				.await
				.expect("seed");
		}
		CheckPolicy::upsert_default(&mut conn, "seedling", "other_source_check")
			.await
			.expect("seed other source");
		CheckPolicy::update(
			&mut conn,
			"alertd",
			"disk_space",
			CheckResult::Failed,
			false,
			None,
			"alice",
		)
		.await
		.expect("update disk_space");
		CheckPolicy::update(
			&mut conn,
			"alertd",
			"chatty",
			CheckResult::Passed,
			false,
			None,
			"alice",
		)
		.await
		.expect("update chatty");

		let map = CheckPolicy::ceiling_map_for_source(&mut conn, "alertd")
			.await
			.expect("map");
		assert_eq!(map.len(), 3, "only the requested source's checks");
		assert_eq!(map.get("disk_space"), Some(&CheckResult::Failed));
		assert_eq!(map.get("cert_expiry"), Some(&CheckResult::Warning));
		assert_eq!(map.get("chatty"), Some(&CheckResult::Passed));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn ceiling_map_ignores_conditional_rules() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::upsert_default(&mut conn, "alertd", "ruled")
			.await
			.expect("seed");
		let ladder: IfLadder = serde_json::from_value(json!({"if": [
			{"==": [{"var": "check.result"}, "failed"]}, "failed",
		]}))
		.expect("parse ladder");
		CheckPolicy::update_rules(&mut conn, "alertd", "ruled", Some(&ladder), "alice")
			.await
			.expect("set rules");

		// The expression could grade a failure through at push time, but
		// the static map must only reflect the ceiling column.
		let map = CheckPolicy::ceiling_map_for_source(&mut conn, "alertd")
			.await
			.expect("map");
		assert_eq!(map.get("ruled"), Some(&CheckResult::Warning));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn silenced_checks_combine_scopes_and_stay_per_source() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group_id = insert_group(&mut conn).await;
		let server_id = insert_server(&mut conn, Some(group_id)).await;
		let other_server_id = insert_server(&mut conn, None).await;

		ServerSilencedRef::add(&mut conn, server_id, "alertd", "health/flaky", None)
			.await
			.expect("server silence");
		ServerGroupSilencedRef::add(&mut conn, group_id, "alertd", "health/groupwide", None)
			.await
			.expect("group silence");
		// None of these may leak into alertd's set: a check's identity is
		// the (source, check) pair, so another source's silence never
		// applies; nor do canopy's own silences or other servers'.
		ServerSilencedRef::add(
			&mut conn,
			server_id,
			"seedling",
			"health/other-source",
			None,
		)
		.await
		.expect("other-source silence");
		ServerSilencedRef::add(&mut conn, server_id, "canopy", "reachability", None)
			.await
			.expect("canopy silence");
		ServerSilencedRef::add(&mut conn, other_server_id, "alertd", "health/other", None)
			.await
			.expect("other-server silence");

		let checks =
			silenced_health_checks_for_server(&mut conn, server_id, Some(group_id), "alertd")
				.await
				.expect("checks");
		assert_eq!(
			checks.into_iter().collect::<Vec<_>>(),
			vec!["flaky", "groupwide"]
		);

		// Ungrouped lookup only sees the server-scope silences.
		let checks = silenced_health_checks_for_server(&mut conn, server_id, None, "alertd")
			.await
			.expect("checks without group");
		assert_eq!(checks.into_iter().collect::<Vec<_>>(), vec!["flaky"]);
	})
	.await
}
