//! DB-layer tests for the managed-restore models (`database::restore`).
//! Exercises the model helpers directly against a fresh migrated DB — no HTTP.

use commons_errors::AppError;
use commons_tests::db::TestDb;
use commons_types::backup::{BackupType, RestoreIntent};
use database::diesel_async::AsyncPgConnection;
use database::pg_duration::PgDuration;
use database::{NewRestoreReplica, RestoreConsumerCapability, RestoreReplica};
use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use jiff::SignedDuration;
use uuid::Uuid;

#[derive(diesel::QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_group(conn: &mut AsyncPgConnection, name: &str) -> Uuid {
	sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(name)
		.get_result::<RowId>(conn)
		.await
		.expect("insert group")
		.id
}

async fn insert_server(conn: &mut AsyncPgConnection, group_id: Uuid) -> Uuid {
	let host = format!("http://test.invalid/{}", Uuid::new_v4());
	sql_query("INSERT INTO servers (host, kind, group_id) VALUES ($1, 'central', $2) RETURNING id")
		.bind::<sql_types::Text, _>(host)
		.bind::<sql_types::Uuid, _>(group_id)
		.get_result::<RowId>(conn)
		.await
		.expect("insert server")
		.id
}

async fn insert_consumer(conn: &mut AsyncPgConnection) -> Uuid {
	sql_query("INSERT INTO devices (role) VALUES ('backup-restore') RETURNING id")
		.get_result::<RowId>(conn)
		.await
		.expect("insert device")
		.id
}

fn new_replica(
	consumer: Uuid,
	group: Uuid,
	server: Option<Uuid>,
	intent: RestoreIntent,
	name: &str,
) -> NewRestoreReplica {
	NewRestoreReplica {
		consumer_device_id: consumer,
		group_id: group,
		server_id: server,
		r#type: BackupType::TamanuPostgres,
		intent,
		name: name.into(),
		freshness: None,
		created_by: Some("op@example.com".into()),
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn create_list_get_roundtrip() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;

		let created = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::Verify, "verify-all"),
		)
		.await
		.expect("create");
		assert_eq!(created.name, "verify-all");
		assert_eq!(created.intent, RestoreIntent::Verify);
		assert!(created.enabled, "new declarations default to enabled");
		assert_eq!(created.created_by.as_deref(), Some("op@example.com"));

		let got = RestoreReplica::get(&mut conn, created.id)
			.await
			.expect("get");
		assert_eq!(got.id, created.id);

		let all = RestoreReplica::list_all(&mut conn).await.expect("list_all");
		assert_eq!(all.len(), 1);

		let for_group = RestoreReplica::list_for_group(&mut conn, group)
			.await
			.expect("list_for_group");
		assert_eq!(for_group.len(), 1);

		let enabled = RestoreReplica::list_enabled_for_consumer(&mut conn, consumer)
			.await
			.expect("list_enabled");
		assert_eq!(enabled.len(), 1);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_scope_conflicts_but_server_scope_is_separate() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;

		RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::Verify, "group-wide"),
		)
		.await
		.expect("group-wide");

		// Same (consumer, group, type, intent) group-wide scope → 409.
		let dup = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::Verify, "dup"),
		)
		.await;
		assert!(matches!(dup, Err(AppError::Conflict(_))), "got {dup:?}");

		// A server-scoped declaration for the same tuple is tracked separately.
		RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				Some(server),
				RestoreIntent::Verify,
				"server-scoped",
			),
		)
		.await
		.expect("server-scoped coexists with group-wide");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_and_delete() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::Verify, "n"),
		)
		.await
		.expect("create");

		let updated = RestoreReplica::update(
			&mut conn,
			r.id,
			"renamed",
			Some(PgDuration(SignedDuration::from_secs(7200))),
			false,
		)
		.await
		.expect("update");
		assert_eq!(updated.name, "renamed");
		assert!(!updated.enabled);
		assert_eq!(updated.freshness.map(|f| f.0.as_secs()), Some(7200));

		// Disabled declarations drop out of the consumer worklist basis.
		let enabled = RestoreReplica::list_enabled_for_consumer(&mut conn, consumer)
			.await
			.expect("list_enabled");
		assert!(enabled.is_empty());

		RestoreReplica::delete(&mut conn, r.id)
			.await
			.expect("delete");
		assert!(RestoreReplica::get(&mut conn, r.id).await.is_err());
		assert!(
			RestoreReplica::delete(&mut conn, r.id).await.is_err(),
			"deleting a missing declaration errors"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn authorizes_only_with_enabled_matching_declaration() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let other_group = insert_group(&mut conn, "other").await;
		let tpg = BackupType::TamanuPostgres;

		assert!(
			!RestoreReplica::authorizes(&mut conn, consumer, group, &tpg)
				.await
				.unwrap(),
			"no declaration → not authorized"
		);

		let r = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::Verify, "n"),
		)
		.await
		.expect("create");

		assert!(
			RestoreReplica::authorizes(&mut conn, consumer, group, &tpg)
				.await
				.unwrap(),
			"enabled declaration → authorized"
		);
		assert!(
			!RestoreReplica::authorizes(&mut conn, consumer, other_group, &tpg)
				.await
				.unwrap(),
			"different group → not authorized"
		);
		assert!(
			!RestoreReplica::authorizes(&mut conn, consumer, group, &BackupType::from("files"))
				.await
				.unwrap(),
			"different type → not authorized"
		);

		// Disabling the only declaration revokes authorization.
		RestoreReplica::update(&mut conn, r.id, "n", None, false)
			.await
			.expect("disable");
		assert!(
			!RestoreReplica::authorizes(&mut conn, consumer, group, &tpg)
				.await
				.unwrap(),
			"disabled declaration → not authorized"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn capability_register_replaces_set() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;

		RestoreConsumerCapability::register(
			&mut conn,
			consumer,
			&[RestoreIntent::Verify, RestoreIntent::Analytics],
		)
		.await
		.expect("register");
		let mut got = RestoreConsumerCapability::list_for_consumer(&mut conn, consumer)
			.await
			.expect("list");
		got.sort_by_key(|i| i.to_string());
		assert_eq!(got, vec![RestoreIntent::Analytics, RestoreIntent::Verify]);

		// Re-register a different set: verify is kept, analytics dropped,
		// disaster-recovery added.
		RestoreConsumerCapability::register(
			&mut conn,
			consumer,
			&[RestoreIntent::Verify, RestoreIntent::DisasterRecovery],
		)
		.await
		.expect("re-register");
		let mut got = RestoreConsumerCapability::list_for_consumer(&mut conn, consumer)
			.await
			.expect("list");
		got.sort_by_key(|i| i.to_string());
		assert_eq!(
			got,
			vec![RestoreIntent::DisasterRecovery, RestoreIntent::Verify]
		);

		// Empty set clears all capabilities.
		RestoreConsumerCapability::register(&mut conn, consumer, &[])
			.await
			.expect("clear");
		let got = RestoreConsumerCapability::list_for_consumer(&mut conn, consumer)
			.await
			.expect("list");
		assert!(got.is_empty());
	})
	.await;
}
