//! Queries backing the device-facing effective check-severity map:
//! `HealthcheckSeverity::base_severity_map` (static catalog severities,
//! ignoring conditional rules) and `silenced_refs::silenced_refs_with_prefix`
//! (server- plus group-scope silences under a source/ref prefix).

use commons_types::issue::Severity;
use database::healthcheck_severities::{HealthcheckSeverity, IfLadder};
use database::silenced_refs::{
	ServerGroupSilencedRef, ServerSilencedRef, silenced_refs_with_prefix,
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
async fn base_severity_map_returns_static_severities() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		for check in ["disk_space", "cert_expiry", "chatty"] {
			HealthcheckSeverity::upsert_default(&mut conn, check)
				.await
				.expect("seed");
		}
		HealthcheckSeverity::update(&mut conn, "disk_space", Severity::Error, None, "alice")
			.await
			.expect("update disk_space");
		HealthcheckSeverity::update(&mut conn, "chatty", Severity::Info, None, "alice")
			.await
			.expect("update chatty");

		let map = HealthcheckSeverity::base_severity_map(&mut conn)
			.await
			.expect("map");
		assert_eq!(map.len(), 3);
		assert_eq!(map.get("disk_space"), Some(&Severity::Error));
		assert_eq!(map.get("cert_expiry"), Some(&Severity::Warning));
		assert_eq!(map.get("chatty"), Some(&Severity::Info));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn base_severity_map_ignores_conditional_rules() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		HealthcheckSeverity::upsert_default(&mut conn, "ruled")
			.await
			.expect("seed");
		let ladder: IfLadder = serde_json::from_value(json!({"if": [
			{"==": [{"var": "check.result"}, "failed"]}, "critical",
		]}))
		.expect("parse ladder");
		HealthcheckSeverity::update_rules(&mut conn, "ruled", Some(&ladder), "alice")
			.await
			.expect("set rules");

		// The expression could raise a failure to critical at push time, but
		// the static map must only reflect the base severity column.
		let map = HealthcheckSeverity::base_severity_map(&mut conn)
			.await
			.expect("map");
		assert_eq!(map.get("ruled"), Some(&Severity::Warning));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn silenced_refs_with_prefix_combines_scopes_and_filters() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group_id = insert_group(&mut conn).await;
		let server_id = insert_server(&mut conn, Some(group_id)).await;
		let other_server_id = insert_server(&mut conn, None).await;

		ServerSilencedRef::add(&mut conn, server_id, "status", "health/flaky", None)
			.await
			.expect("server silence");
		ServerGroupSilencedRef::add(&mut conn, group_id, "status", "health/groupwide", None)
			.await
			.expect("group silence");
		// None of these may leak into the result: wrong source, wrong ref
		// prefix (broken issues are a separate thread), wrong server.
		ServerSilencedRef::add(&mut conn, server_id, "canopy", "health/wrong-source", None)
			.await
			.expect("other-source silence");
		ServerSilencedRef::add(&mut conn, server_id, "status", "health-broken/flaky", None)
			.await
			.expect("broken silence");
		ServerSilencedRef::add(&mut conn, other_server_id, "status", "health/other", None)
			.await
			.expect("other-server silence");

		let mut refs =
			silenced_refs_with_prefix(&mut conn, server_id, Some(group_id), "status", "health/")
				.await
				.expect("refs");
		refs.sort();
		assert_eq!(refs, vec!["health/flaky", "health/groupwide"]);

		// Ungrouped lookup only sees the server-scope silences.
		let refs = silenced_refs_with_prefix(&mut conn, server_id, None, "status", "health/")
			.await
			.expect("refs without group");
		assert_eq!(refs, vec!["health/flaky"]);
	})
	.await
}
