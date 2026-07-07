//! `silenced_refs::silenced_health_checks_for_server(s)`: resolving the
//! set of healthcheck names silenced for a server from its `(status,
//! health/<check>)` silence entries, at server and group scope, in one
//! batch. This set feeds `Status::health_state_ignoring` so silenced
//! checks don't count toward the health rollup.

use std::collections::BTreeSet;

use commons_tests::db::TestDb;
use database::silenced_refs::{
	ServerGroupSilencedRef, ServerSilencedRef, silenced_health_checks_for_server,
	silenced_health_checks_for_servers,
};
use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

async fn insert_group(conn: &mut database::diesel_async::AsyncPgConnection, name: &str) -> Uuid {
	#[derive(diesel::QueryableByName)]
	struct RowId {
		#[diesel(sql_type = sql_types::Uuid)]
		id: Uuid,
	}
	let row: RowId = sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(name)
		.get_result(conn)
		.await
		.expect("insert group");
	row.id
}

async fn insert_server(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group_id: Option<Uuid>,
) -> Uuid {
	#[derive(diesel::QueryableByName)]
	struct RowId {
		#[diesel(sql_type = sql_types::Uuid)]
		id: Uuid,
	}
	let host = format!("http://test.invalid/{}", Uuid::new_v4());
	let row: RowId = sql_query(
		"INSERT INTO servers (host, kind, group_id) VALUES ($1, 'central', $2) RETURNING id",
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group_id)
	.get_result(conn)
	.await
	.expect("insert server");
	row.id
}

fn checks(names: &[&str]) -> BTreeSet<String> {
	names.iter().map(|s| s.to_string()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn resolves_server_and_group_scopes_in_batch() {
	TestDb::run(async |mut conn, _url| {
		let group = insert_group(&mut conn, "g").await;
		let grouped = insert_server(&mut conn, Some(group)).await;
		let ungrouped = insert_server(&mut conn, None).await;
		let unsilenced = insert_server(&mut conn, Some(group)).await;

		ServerSilencedRef::add(&mut conn, grouped, "status", "health/postgres", None)
			.await
			.unwrap();
		ServerSilencedRef::add(&mut conn, ungrouped, "status", "health/disk", None)
			.await
			.unwrap();
		ServerGroupSilencedRef::add(&mut conn, group, "status", "health/uploads", None)
			.await
			.unwrap();

		let map = silenced_health_checks_for_servers(
			&mut conn,
			&[
				(grouped, Some(group)),
				(ungrouped, None),
				(unsilenced, Some(group)),
			],
		)
		.await
		.unwrap();

		// Server scope and group scope combine for the grouped server.
		assert_eq!(map.get(&grouped), Some(&checks(&["postgres", "uploads"])));
		// The ungrouped server only sees its own silences.
		assert_eq!(map.get(&ungrouped), Some(&checks(&["disk"])));
		// A group member with no server-scope silence still inherits the
		// group's.
		assert_eq!(map.get(&unsilenced), Some(&checks(&["uploads"])));

		// The single-server convenience agrees.
		assert_eq!(
			silenced_health_checks_for_server(&mut conn, grouped, Some(group))
				.await
				.unwrap(),
			checks(&["postgres", "uploads"]),
		);
	})
	.await
}

/// Only `(status, health/<check>)` silences are healthcheck silences:
/// other sources and non-health refs (e.g. canopy reachability) don't
/// leak into the set.
#[tokio::test(flavor = "multi_thread")]
async fn ignores_non_healthcheck_silences() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, None).await;

		ServerSilencedRef::add(&mut conn, server, "canopy", "reachability", None)
			.await
			.unwrap();
		ServerSilencedRef::add(&mut conn, server, "backups", "health/postgres", None)
			.await
			.unwrap();
		ServerSilencedRef::add(&mut conn, server, "status", "something-else", None)
			.await
			.unwrap();

		let map = silenced_health_checks_for_servers(&mut conn, &[(server, None)])
			.await
			.unwrap();
		assert_eq!(map.get(&server), None);
	})
	.await
}

/// Removing a silence takes the check back out of the set.
#[tokio::test(flavor = "multi_thread")]
async fn unsilencing_removes_the_check() {
	TestDb::run(async |mut conn, _url| {
		let server = insert_server(&mut conn, None).await;

		ServerSilencedRef::add(&mut conn, server, "status", "health/postgres", None)
			.await
			.unwrap();
		assert_eq!(
			silenced_health_checks_for_server(&mut conn, server, None)
				.await
				.unwrap(),
			checks(&["postgres"]),
		);

		ServerSilencedRef::remove(&mut conn, server, "status", "health/postgres")
			.await
			.unwrap();
		assert_eq!(
			silenced_health_checks_for_server(&mut conn, server, None)
				.await
				.unwrap(),
			BTreeSet::new(),
		);
	})
	.await
}
