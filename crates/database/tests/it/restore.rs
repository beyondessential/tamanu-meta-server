//! DB-layer tests for the managed-restore models (`database::restore`).
//! Exercises the model helpers directly against a fresh migrated DB — no HTTP.

use commons_errors::AppError;
use commons_tests::db::TestDb;
use commons_types::backup::{BackupType, IntentDescriptor, RestoreIntent, RunOutcome};
use database::diesel_async::AsyncPgConnection;
use database::pg_duration::PgDuration;
use database::{
	BackupRestoreCheck, NewBackupRestoreCheck, NewRestoreReplica, RestoreConsumerCapability,
	RestoreReplica, RestoreReplicaUpdate,
};
use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

#[derive(diesel::QueryableByName)]
struct Count {
	#[diesel(sql_type = sql_types::BigInt)]
	count: i64,
}

/// Count active `restore-verification` check-states across a group's servers.
///
/// Sweeps first: `sweep_overdue` is the sole filer of the restore checks and
/// rebuilds each server's from its live declarations, so what a recorded report
/// or a deleted declaration did shows up on the next pass rather than at the
/// moment it happened.
async fn active_restore_issues(conn: &mut AsyncPgConnection, group: Uuid) -> i64 {
	database::restore::sweep_overdue(conn).await.expect("sweep");
	sql_query(
		"SELECT count(*) AS count FROM issues i \
		 JOIN servers s ON s.id = i.server_id \
		 WHERE s.group_id = $1 AND i.ref = 'restore-verification' AND i.active = true",
	)
	.bind::<sql_types::Uuid, _>(group)
	.get_result::<Count>(conn)
	.await
	.expect("count issues")
	.count
}

fn new_check(
	consumer: Uuid,
	group: Uuid,
	server: Uuid,
	intent: RestoreIntent,
	outcome: RunOutcome,
	healthy: bool,
) -> NewBackupRestoreCheck {
	NewBackupRestoreCheck {
		replica_id: None,
		consumer_device_id: consumer,
		group_id: group,
		server_id: Some(server),
		r#type: BackupType::TamanuPostgres,
		intent,
		snapshot_id: Some("snap-x".into()),
		outcome,
		error: None,
		replica_healthy: healthy,
		postgres_version: Some("15".into()),
		observed_at: Timestamp::now(),
		s3_sent_raw_bytes: None,
		s3_sent_payload_bytes: None,
		s3_received_raw_bytes: None,
		s3_received_payload_bytes: None,
		health_details: None,
		run_id: None,
		redaction_outcome: None,
		redaction_manifest_version: None,
		redaction_columns_masked: None,
		redaction_columns_skipped: None,
		redaction_error: None,
	}
}

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
		overdue_after: None,
		params: serde_json::json!({}),
		redacts: false,
		created_by: Some("op@example.com".into()),
	}
}

/// An update payload that carries `r`'s current fields forward unchanged,
/// so a test only needs to spell out the fields it's actually changing.
fn update_from(r: &RestoreReplica) -> RestoreReplicaUpdate {
	RestoreReplicaUpdate {
		consumer_device_id: r.consumer_device_id,
		group_id: r.group_id,
		server_id: r.server_id,
		r#type: r.r#type.clone(),
		intent: r.intent.clone(),
		name: r.name.clone(),
		overdue_after: r.overdue_after,
		params: r.params.clone(),
		redacts: r.redacts,
		enabled: r.enabled,
	}
}

/// A minimal capability descriptor advertising the given intent and semantics.
fn descriptor(intent: &str, semantics: &[&str]) -> IntentDescriptor {
	IntentDescriptor {
		intent: RestoreIntent::from(intent),
		description: None,
		semantics: semantics.iter().map(|s| (*s).to_owned()).collect(),
		params: Default::default(),
	}
}

/// Declare the replica a report is about, group-wide. The ingest only accepts a
/// report a declaration authorizes, and the sweep only derives instances for
/// declared replicas, so a report standing on its own is a state production
/// cannot reach.
async fn declare(conn: &mut AsyncPgConnection, consumer: Uuid, group: Uuid, intent: &str) -> Uuid {
	RestoreReplica::create(
		conn,
		new_replica(
			consumer,
			group,
			None,
			RestoreIntent::from(intent),
			&format!("{intent}-all"),
		),
	)
	.await
	.expect("declare replica")
	.id
}

#[tokio::test(flavor = "multi_thread")]
async fn create_list_get_roundtrip() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;

		let created = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				None,
				RestoreIntent::from("verify"),
				"verify-all",
			),
		)
		.await
		.expect("create");
		assert_eq!(created.name, "verify-all");
		assert_eq!(created.intent, RestoreIntent::from("verify"));
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
			new_replica(
				consumer,
				group,
				None,
				RestoreIntent::from("verify"),
				"group-wide",
			),
		)
		.await
		.expect("group-wide");

		// Same (consumer, group, type, intent) group-wide scope → 409.
		let dup = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::from("verify"), "dup"),
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
				RestoreIntent::from("verify"),
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
			new_replica(consumer, group, None, RestoreIntent::from("verify"), "n"),
		)
		.await
		.expect("create");

		let updated = RestoreReplica::update(
			&mut conn,
			r.id,
			RestoreReplicaUpdate {
				name: "renamed".into(),
				overdue_after: Some(PgDuration(SignedDuration::from_secs(7200))),
				params: serde_json::json!({"minimum_uptime": 60}),
				enabled: false,
				..update_from(&r)
			},
		)
		.await
		.expect("update");
		assert_eq!(updated.name, "renamed");
		assert!(!updated.enabled);
		assert_eq!(updated.overdue_after.map(|f| f.0.as_secs()), Some(7200));
		assert_eq!(updated.params, serde_json::json!({"minimum_uptime": 60}));

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
			new_replica(consumer, group, None, RestoreIntent::from("verify"), "n"),
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
		RestoreReplica::update(
			&mut conn,
			r.id,
			RestoreReplicaUpdate {
				enabled: false,
				..update_from(&r)
			},
		)
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
			&[
				descriptor("verify", &["check", "once"]),
				descriptor("analytics", &["check", "url"]),
			],
		)
		.await
		.expect("register");
		let mut got = RestoreConsumerCapability::list_for_consumer(&mut conn, consumer)
			.await
			.expect("list");
		got.sort_by_key(|d| d.intent.to_string());
		let intents: Vec<RestoreIntent> = got.iter().map(|d| d.intent.clone()).collect();
		assert_eq!(
			intents,
			vec![
				RestoreIntent::from("analytics"),
				RestoreIntent::from("verify")
			]
		);
		// The advertised description/semantics/params round-trip.
		let verify = got.iter().find(|d| d.intent.as_str() == "verify").unwrap();
		assert!(verify.has_semantic("once"));
		assert!(verify.has_semantic("check"));

		// Re-register a different set: verify is kept (and its semantics updated),
		// analytics dropped, files added.
		RestoreConsumerCapability::register(
			&mut conn,
			consumer,
			&[
				descriptor("verify", &["check"]),
				descriptor("files", &["check", "url"]),
			],
		)
		.await
		.expect("re-register");
		let mut got = RestoreConsumerCapability::list_for_consumer(&mut conn, consumer)
			.await
			.expect("list");
		got.sort_by_key(|d| d.intent.to_string());
		let intents: Vec<RestoreIntent> = got.iter().map(|d| d.intent.clone()).collect();
		assert_eq!(
			intents,
			vec![RestoreIntent::from("files"), RestoreIntent::from("verify")]
		);
		// Upsert updated verify's semantics (once dropped).
		let verify = got.iter().find(|d| d.intent.as_str() == "verify").unwrap();
		assert!(
			!verify.has_semantic("once"),
			"re-register updates semantics"
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

#[tokio::test(flavor = "multi_thread")]
async fn record_report_raises_then_recovers() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		declare(&mut conn, consumer, group, "verify").await;

		// A failed report degrades the server's restore-verification check.
		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Failure,
				false,
			),
		)
		.await
		.expect("record failure");
		assert_eq!(active_restore_issues(&mut conn, group).await, 1);

		// A success-and-healthy report for the same key recovers it.
		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Success,
				true,
			),
		)
		.await
		.expect("record success");
		assert_eq!(active_restore_issues(&mut conn, group).await, 0);
	})
	.await;
}

#[derive(diesel::QueryableByName)]
struct VerifRow {
	#[diesel(sql_type = sql_types::Text)]
	check_name: String,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
	server_id: Option<Uuid>,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
	server_group_id: Option<Uuid>,
}

#[tokio::test(flavor = "multi_thread")]
async fn record_report_files_server_scoped_with_stable_name() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		declare(&mut conn, consumer, group, "verify").await;

		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Failure,
				false,
			),
		)
		.await
		.expect("record failure");
		database::restore::sweep_overdue(&mut conn)
			.await
			.expect("sweep");

		// The check is server-scoped and named for the condition alone: the
		// server is the scope (issues.server_id) and the replica an instance,
		// neither of them baked into the check name.
		let rows: Vec<VerifRow> = sql_query(
			"SELECT check_name, server_id, server_group_id FROM issues \
			 WHERE source = 'canopy' AND ref = 'restore-verification' AND active",
		)
		.load(&mut conn)
		.await
		.expect("load");
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].check_name, "restore-verification");
		assert_eq!(rows[0].server_id, Some(server));
		assert_eq!(rows[0].server_group_id, None);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn record_report_unhealthy_success_still_raises() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		declare(&mut conn, consumer, group, "verify").await;

		// Restore succeeded but the database wasn't healthy → still a failure.
		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Success,
				false,
			),
		)
		.await
		.expect("record");
		assert_eq!(active_restore_issues(&mut conn, group).await, 1);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_overdue_raises_for_stale_replica_but_skips_gaps() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		insert_server(&mut conn, group).await;

		// Consumer advertises only `verify`, as a standing (non-`once`) check.
		RestoreConsumerCapability::register(
			&mut conn,
			consumer,
			&[descriptor("verify", &["check"])],
		)
		.await
		.expect("register caps");

		// A supported, bounded declaration with no healthy check → overdue.
		let mut verify = new_replica(
			consumer,
			group,
			None,
			RestoreIntent::from("verify"),
			"verify-all",
		);
		verify.overdue_after = Some(PgDuration(SignedDuration::from_secs(3600)));
		RestoreReplica::create(&mut conn, verify)
			.await
			.expect("verify decl");

		// An unadvertised-intent declaration (a gap) must NOT raise.
		let mut analytics = new_replica(
			consumer,
			group,
			None,
			RestoreIntent::from("analytics"),
			"analytics-all",
		);
		analytics.overdue_after = Some(PgDuration(SignedDuration::from_secs(3600)));
		RestoreReplica::create(&mut conn, analytics)
			.await
			.expect("analytics decl");

		let filed = database::restore::sweep_overdue(&mut conn)
			.await
			.expect("sweep");
		assert_eq!(filed, 1, "only the supported declaration is overdue");
		assert_eq!(active_restore_issues(&mut conn, group).await, 1);
	})
	.await;
}

/// Insert a successful backup run whose snapshot was produced `hours_ago`.
async fn insert_old_success_run(
	conn: &mut AsyncPgConnection,
	device: Uuid,
	group: Uuid,
	server: Uuid,
	snapshot: &str,
	hours_ago: i64,
) {
	sql_query(
		"INSERT INTO backup_runs (id, device_id, group_id, server_id, type, purpose, outcome, snapshot_id, reported_at) \
		 VALUES (gen_random_uuid(), $1, $2, $3, 'tamanu-postgres', 'backup', 'success', $4, now() - make_interval(hours => $5))",
	)
	.bind::<sql_types::Uuid, _>(device)
	.bind::<sql_types::Uuid, _>(group)
	.bind::<sql_types::Uuid, _>(server)
	.bind::<sql_types::Text, _>(snapshot)
	.bind::<sql_types::Int4, _>(hours_ago as i32)
	.execute(conn)
	.await
	.expect("insert old run");
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_once_is_snapshot_driven() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;

		RestoreConsumerCapability::register(
			&mut conn,
			consumer,
			&[descriptor("verify", &["check", "once"])],
		)
		.await
		.expect("register caps");

		let mut verify = new_replica(
			consumer,
			group,
			Some(server),
			RestoreIntent::from("verify"),
			"verify-srv",
		);
		verify.overdue_after = Some(PgDuration(SignedDuration::from_secs(3600)));
		RestoreReplica::create(&mut conn, verify)
			.await
			.expect("verify decl");

		// No snapshot exists yet → nothing to verify, so not overdue.
		assert_eq!(
			database::restore::sweep_overdue(&mut conn).await.unwrap(),
			0,
			"no snapshot → not overdue"
		);

		// A snapshot older than the bound, never verified → overdue.
		insert_old_success_run(&mut conn, consumer, group, server, "snap-1", 2).await;
		assert_eq!(
			database::restore::sweep_overdue(&mut conn).await.unwrap(),
			1,
			"old unverified snapshot → overdue"
		);

		// Once that snapshot is verified healthy, the `once` intent is satisfied
		// and no longer overdue (and the alert recovers).
		BackupRestoreCheck::record_report(
			&mut conn,
			NewBackupRestoreCheck {
				snapshot_id: Some("snap-1".into()),
				..new_check(
					consumer,
					group,
					server,
					RestoreIntent::from("verify"),
					RunOutcome::Success,
					true,
				)
			},
		)
		.await
		.expect("record healthy");
		assert_eq!(
			database::restore::sweep_overdue(&mut conn).await.unwrap(),
			0,
			"verified latest snapshot → not overdue"
		);
		assert_eq!(active_restore_issues(&mut conn, group).await, 0);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn records_and_returns_arbitrary_health_details() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;

		let details = serde_json::json!({
			"cluster": { "live_tuples": 12345, "dead_tuples": 6 },
			"indexes_fixed": true,
		});
		let mut check = new_check(
			consumer,
			group,
			server,
			RestoreIntent::from("verify"),
			RunOutcome::Success,
			true,
		);
		check.health_details = Some(details.clone());
		BackupRestoreCheck::record_report(&mut conn, check)
			.await
			.expect("record");

		let recent = BackupRestoreCheck::list_recent_for_group(&mut conn, group, 10)
			.await
			.expect("list");
		assert_eq!(recent.len(), 1);
		// Stored and returned verbatim (opaque to canopy).
		assert_eq!(recent[0].health_details, Some(details));

		// A report with no health data keeps the column NULL.
		let plain = new_check(
			consumer,
			group,
			server,
			RestoreIntent::from("verify"),
			RunOutcome::Success,
			true,
		);
		BackupRestoreCheck::record_report(&mut conn, plain)
			.await
			.expect("record plain");
		let recent = BackupRestoreCheck::list_recent_for_group(&mut conn, group, 10)
			.await
			.expect("list");
		assert!(recent.iter().any(|c| c.health_details.is_none()));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_can_change_scope_including_intent() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::from("verify"), "n"),
		)
		.await
		.expect("create");

		let updated = RestoreReplica::update(
			&mut conn,
			r.id,
			RestoreReplicaUpdate {
				intent: RestoreIntent::from("analytics"),
				..update_from(&r)
			},
		)
		.await
		.expect("update intent");
		assert_eq!(updated.intent, RestoreIntent::from("analytics"));

		let got = RestoreReplica::get(&mut conn, r.id).await.expect("get");
		assert_eq!(got.intent, RestoreIntent::from("analytics"), "persisted");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_scope_collision_conflicts() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::from("verify"), "a"),
		)
		.await
		.expect("create a");
		let b = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::from("analytics"), "b"),
		)
		.await
		.expect("create b");

		// Retargeting b's intent onto a's scope collides with a's group-wide
		// declaration.
		let result = RestoreReplica::update(
			&mut conn,
			b.id,
			RestoreReplicaUpdate {
				intent: RestoreIntent::from("verify"),
				..update_from(&b)
			},
		)
		.await;
		assert!(
			matches!(result, Err(AppError::Conflict(_))),
			"got {result:?}"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_moving_scope_recovers_stale_alert_at_old_key() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				Some(server),
				RestoreIntent::from("verify"),
				"n",
			),
		)
		.await
		.expect("create");

		// Raise an active alert at the declaration's (server, type, verify) key.
		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Failure,
				false,
			),
		)
		.await
		.expect("record failure");
		assert_eq!(active_restore_issues(&mut conn, group).await, 1);

		// Retargeting the declaration's intent moves it off that key. The sweep
		// only walks current declarations, so without recovery this alert would
		// never clear.
		RestoreReplica::update(
			&mut conn,
			r.id,
			RestoreReplicaUpdate {
				intent: RestoreIntent::from("analytics"),
				..update_from(&r)
			},
		)
		.await
		.expect("retarget intent");
		assert_eq!(
			active_restore_issues(&mut conn, group).await,
			0,
			"old-key alert is recovered on scope change"
		);
	})
	.await;
}

/// Disabling a declaration drops it out of the overdue sweep exactly as
/// deleting it does, and a disabled replica generates no consumer work, so
/// nothing can clear its alert. Decommissioning by disabling must not leave
/// the operator paged forever.
#[tokio::test(flavor = "multi_thread")]
async fn disabling_recovers_the_stale_alert() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				Some(server),
				RestoreIntent::from("verify"),
				"n",
			),
		)
		.await
		.expect("create");

		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Failure,
				false,
			),
		)
		.await
		.expect("record failure");
		assert_eq!(active_restore_issues(&mut conn, group).await, 1);

		RestoreReplica::update(
			&mut conn,
			r.id,
			RestoreReplicaUpdate {
				enabled: false,
				..update_from(&r)
			},
		)
		.await
		.expect("disable");
		assert_eq!(
			active_restore_issues(&mut conn, group).await,
			0,
			"a disabled declaration's alert is recovered, not left open forever"
		);
	})
	.await;
}

/// Re-enabling is not a recovery event of its own: the sweep picks the
/// declaration back up and the replica's latest report is still the last word on
/// it, so a finding that stood before the replica was decommissioned stands
/// again after it is put back.
#[tokio::test(flavor = "multi_thread")]
async fn re_enabling_does_not_recover_anything() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				Some(server),
				RestoreIntent::from("verify"),
				"n",
			),
		)
		.await
		.expect("create");

		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Failure,
				false,
			),
		)
		.await
		.expect("record failure");
		assert_eq!(active_restore_issues(&mut conn, group).await, 1);

		let disabled = RestoreReplica::update(
			&mut conn,
			r.id,
			RestoreReplicaUpdate {
				enabled: false,
				..update_from(&r)
			},
		)
		.await
		.expect("disable");
		assert_eq!(
			active_restore_issues(&mut conn, group).await,
			0,
			"decommissioning the replica takes its finding with it"
		);

		RestoreReplica::update(
			&mut conn,
			disabled.id,
			RestoreReplicaUpdate {
				enabled: true,
				..update_from(&disabled)
			},
		)
		.await
		.expect("re-enable");
		assert_eq!(
			active_restore_issues(&mut conn, group).await,
			1,
			"re-enabling is not a recovery: the failed report still stands"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_recovers_stale_alert_for_removed_scope() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				Some(server),
				RestoreIntent::from("verify"),
				"n",
			),
		)
		.await
		.expect("create");

		// Raise an active alert at the declaration's (server, type, verify) key.
		BackupRestoreCheck::record_report(
			&mut conn,
			new_check(
				consumer,
				group,
				server,
				RestoreIntent::from("verify"),
				RunOutcome::Failure,
				false,
			),
		)
		.await
		.expect("record failure");
		assert_eq!(active_restore_issues(&mut conn, group).await, 1);

		// Deleting the declaration removes the only thing tracking that
		// replica, so the next sweep rebuilds the server's check without it.
		RestoreReplica::delete(&mut conn, r.id)
			.await
			.expect("delete");
		assert_eq!(
			active_restore_issues(&mut conn, group).await,
			0,
			"the removed scope's alert is recovered on delete"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_group_wide_recovers_alerts_on_every_server() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server_a = insert_server(&mut conn, group).await;
		let server_b = insert_server(&mut conn, group).await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::from("verify"), "n"),
		)
		.await
		.expect("create");

		for sid in [server_a, server_b] {
			BackupRestoreCheck::record_report(
				&mut conn,
				new_check(
					consumer,
					group,
					sid,
					RestoreIntent::from("verify"),
					RunOutcome::Failure,
					false,
				),
			)
			.await
			.expect("record failure");
		}
		assert_eq!(active_restore_issues(&mut conn, group).await, 2);

		RestoreReplica::delete(&mut conn, r.id)
			.await
			.expect("delete");
		assert_eq!(
			active_restore_issues(&mut conn, group).await,
			0,
			"a group-wide declaration's alerts recover on every live server"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_detaches_reports_rather_than_being_blocked_by_them() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;
		let server = insert_server(&mut conn, group).await;
		let r = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				Some(server),
				RestoreIntent::from("verify"),
				"n",
			),
		)
		.await
		.expect("create");

		BackupRestoreCheck::record_report(
			&mut conn,
			NewBackupRestoreCheck {
				replica_id: Some(r.id),
				..new_check(
					consumer,
					group,
					server,
					RestoreIntent::from("verify"),
					RunOutcome::Success,
					true,
				)
			},
		)
		.await
		.expect("record report");

		// A reported-on declaration is no harder to retire than one that never
		// was: the report's reference is cleared instead of pinning the row.
		RestoreReplica::delete(&mut conn, r.id)
			.await
			.expect("a declaration with restore-health history deletes");

		let checks = BackupRestoreCheck::list_recent_for_group(&mut conn, group, 10)
			.await
			.expect("list checks");
		assert_eq!(checks.len(), 1, "the report is retained");
		assert_eq!(
			checks[0].replica_id, None,
			"the retained report no longer names a declaration"
		);
		assert_eq!(checks[0].server_id, Some(server));
		assert_eq!(checks[0].intent, RestoreIntent::from("verify"));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn name_is_unique_per_consumer() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let other_consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;

		let first = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				None,
				RestoreIntent::from("verify"),
				"daily",
			),
		)
		.await
		.expect("create");

		// A different intent is a different scope, so the scope indexes allow
		// it — but reusing the name would leave the consumer with two replicas
		// it can't tell apart.
		let dup = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				None,
				RestoreIntent::from("standby"),
				"daily",
			),
		)
		.await;
		assert!(matches!(dup, Err(AppError::Conflict(_))), "got {dup:?}");

		// Surrounding whitespace doesn't buy a way around it.
		let padded = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				None,
				RestoreIntent::from("standby"),
				"  daily  ",
			),
		)
		.await;
		assert!(
			matches!(padded, Err(AppError::Conflict(_))),
			"got {padded:?}"
		);

		// Another consumer's declarations are named independently.
		RestoreReplica::create(
			&mut conn,
			new_replica(
				other_consumer,
				group,
				None,
				RestoreIntent::from("verify"),
				"daily",
			),
		)
		.await
		.expect("a different consumer may reuse the name");

		// Renaming onto a sibling's name is refused the same way.
		let second = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				None,
				RestoreIntent::from("standby"),
				"weekly",
			),
		)
		.await
		.expect("create second");
		let clash = RestoreReplica::update(
			&mut conn,
			second.id,
			RestoreReplicaUpdate {
				name: first.name.clone(),
				..update_from(&second)
			},
		)
		.await;
		assert!(matches!(clash, Err(AppError::Conflict(_))), "got {clash:?}");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn blank_name_is_rejected() {
	TestDb::run(|mut conn, _url| async move {
		let consumer = insert_consumer(&mut conn).await;
		let group = insert_group(&mut conn, "g").await;

		let blank = RestoreReplica::create(
			&mut conn,
			new_replica(consumer, group, None, RestoreIntent::from("verify"), "   "),
		)
		.await;
		assert!(
			matches!(blank, Err(AppError::BadRequest(_))),
			"got {blank:?}"
		);

		let r = RestoreReplica::create(
			&mut conn,
			new_replica(
				consumer,
				group,
				None,
				RestoreIntent::from("verify"),
				"  padded  ",
			),
		)
		.await
		.expect("create");
		assert_eq!(r.name, "padded", "the stored name is trimmed");

		let blanked = RestoreReplica::update(
			&mut conn,
			r.id,
			RestoreReplicaUpdate {
				name: "".into(),
				..update_from(&r)
			},
		)
		.await;
		assert!(
			matches!(blanked, Err(AppError::BadRequest(_))),
			"got {blanked:?}"
		);
	})
	.await;
}

/// Canopy corroborates a product's manifest template against what each
/// version actually published, so a redacting declaration pointing at a
/// version with no manifest is a finding before any restore is attempted.
#[tokio::test(flavor = "multi_thread")]
async fn a_version_without_a_published_manifest_is_a_redaction_gap() {
	TestDb::run(|mut conn, _url| async move {
		let group = insert_group(&mut conn, "redaction-gap").await;
		let server_id = insert_server(&mut conn, group).await;
		let server = database::servers::Server::get_by_id(&mut conn, server_id)
			.await
			.expect("server");

		// No version reported yet: canopy can't corroborate, and the consumer
		// resolves the manifest against the data it restores anyway.
		assert!(
			database::restore::redaction_gap_for(&mut conn, &server)
				.await
				.expect("gap")
				.is_none(),
			"an unknown version is not a server that can't be redacted"
		);

		let version_id = sql_query(
			"INSERT INTO versions (major, minor, patch, status, changelog) \
			 VALUES (2, 41, 3, 'published', '') RETURNING id",
		)
		.get_result::<RowId>(&mut conn)
		.await
		.expect("insert version")
		.id;
		sql_query(
			"INSERT INTO server_reported_detail (server_id, source, extra, version) \
			 VALUES ($1, 'tamanu', '{}'::jsonb, '2.41.3')",
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.execute(&mut conn)
		.await
		.expect("report a version");

		let gap = database::restore::redaction_gap_for(&mut conn, &server)
			.await
			.expect("gap");
		assert_eq!(
			gap,
			Some((
				database::restore::RedactionGapReason::VersionHasNoManifest,
				Some("2.41.3".to_string())
			)),
			"a version that published no manifest is a gap"
		);

		sql_query(
			"INSERT INTO artifacts (version_id, platform, artifact_type, download_url) \
			 VALUES ($1, 'any', 'dbt-manifest', 'https://docs.example/manifest.json')",
		)
		.bind::<sql_types::Uuid, _>(version_id)
		.execute(&mut conn)
		.await
		.expect("publish a manifest");

		assert!(
			database::restore::redaction_gap_for(&mut conn, &server)
				.await
				.expect("gap")
				.is_none(),
			"a version whose manifest is published is no gap"
		);
	})
	.await;
}
