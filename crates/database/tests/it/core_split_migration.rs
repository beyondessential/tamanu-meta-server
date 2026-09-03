//! The core-model split lands as a run of migrations, starting with
//! `2026-08-26-101808-0000_rename_servers_to_applications`. Between them they
//! turn each pre-split `servers` row into an application and the box it runs
//! on, and move the box's facts onto the box.
//!
//! Replays that for real: reverts back past the rename, seeds servers in the
//! shape they had before it, then re-applies the whole run forward.

use diesel::sql_types;
use diesel_async::{
	AsyncConnection as _, AsyncMigrationHarness, AsyncPgConnection, RunQueryDsl,
	SimpleAsyncConnection as _,
};
use diesel_migrations::MigrationHarness as _;
use uuid::Uuid;

/// The first migration of the split, `rename_servers_to_applications`, in the
/// digits-only form diesel stores a version as. Reverting to before it puts
/// the database back in the shape a deployment had on the day this card
/// started.
const FIRST_SPLIT_MIGRATION: &str = "202608261018080000";

/// Canopy's own record of itself, which predates the split and is not a box in
/// the field. Fleet queries skip it and so does this.
const NIL: &str = "00000000-0000-0000-0000-000000000000";

#[derive(diesel::QueryableByName)]
struct Count {
	#[diesel(sql_type = sql_types::BigInt)]
	n: i64,
}

/// Revert every migration this card added, back to the day before the rename.
async fn revert_the_split(url: &str) {
	let conn = AsyncPgConnection::establish(url)
		.await
		.expect("second connection");
	let mut harness = AsyncMigrationHarness::new(conn);
	loop {
		let applied = harness
			.applied_migrations()
			.expect("read the applied migrations");
		let latest = applied
			.iter()
			.map(ToString::to_string)
			.max()
			.expect("some migration is applied");
		if latest.as_str() < FIRST_SPLIT_MIGRATION {
			break;
		}
		harness
			.revert_last_migration(commons_tests::db::MIGRATIONS)
			.unwrap_or_else(|err| panic!("revert {latest}: {err}"));
	}
}

/// Re-apply the split over the seeded pre-split data.
async fn apply_the_split(url: &str) {
	let conn = AsyncPgConnection::establish(url)
		.await
		.expect("second connection");
	AsyncMigrationHarness::new(conn)
		.run_pending_migrations(commons_tests::db::MIGRATIONS)
		.unwrap_or_else(|err| panic!("re-apply: {err}"));
}

/// One server becomes one application on one machine, the box's facts land on
/// the box, and the check states, silences and incidents that named the server
/// still name the application.
// spec: FLT
#[tokio::test(flavor = "multi_thread")]
async fn the_split_gives_every_server_a_machine_and_leaves_its_history_alone() {
	commons_tests::db::TestDb::run(async |mut conn, url| {
		revert_the_split(&url).await;

		conn.batch_execute(
			"INSERT INTO server_groups (id, name) \
			 VALUES ('11111111-1111-1111-1111-111111111111', 'a-deployment');

			 INSERT INTO devices (id, role, tailscale_node_id) \
			 VALUES ('22222222-2222-2222-2222-222222222222', 'server', 'node-one');

			 INSERT INTO servers \
			   (id, name, host, product, kind, rank, group_id, device_id, \
			    alert_when_down_for, cloud, is_monitored) \
			 VALUES \
			   ('33333333-3333-3333-3333-333333333333', 'box-one', \
			    'https://one.invalid/', 'tamanu', 'central', 'production', \
			    '11111111-1111-1111-1111-111111111111', \
			    '22222222-2222-2222-2222-222222222222', \
			    INTERVAL '17 minutes', true, true), \
			   ('44444444-4444-4444-4444-444444444444', 'box-two', \
			    'https://two.invalid/', 'tamanu', 'facility', 'production', \
			    NULL, NULL, INTERVAL '5 minutes', NULL, true);

			 INSERT INTO issues \
			   (id, server_id, source, \"ref\", message, check_name, \
			    observed_result, effective_result, active) \
			 VALUES ('55555555-5555-5555-5555-555555555555', \
			         '33333333-3333-3333-3333-333333333333', 'alertd', \
			         'check:db', 'the database is unreachable', 'db', \
			         'failed', 'failed', true);

			 INSERT INTO scoped_check_policies \
			   (id, source, check_name, server_id, ceiling, created_by) \
			 VALUES ('66666666-6666-6666-6666-666666666666', 'alertd', 'db', \
			         '33333333-3333-3333-3333-333333333333', 'warning', \
			         'someone@example.com');

			 INSERT INTO incidents (id, opened_at) \
			 VALUES ('77777777-7777-7777-7777-777777777777', NOW());

			 INSERT INTO incident_issues (incident_id, issue_id, joined_at) \
			 VALUES ('77777777-7777-7777-7777-777777777777', \
			         '55555555-5555-5555-5555-555555555555', NOW());",
		)
		.await
		.expect("seed the pre-split shape");

		apply_the_split(&url).await;

		// Every server became one application, and every application one box.
		#[derive(diesel::QueryableByName)]
		struct Pairing {
			#[diesel(sql_type = sql_types::Text)]
			name: String,
			#[diesel(sql_type = sql_types::Text)]
			type_: String,
			#[diesel(sql_type = sql_types::Uuid)]
			machine_id: Uuid,
			#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
			machine_name: Option<String>,
		}
		let pairs: Vec<Pairing> = diesel::sql_query(
			"SELECT a.name, a.type AS type_, a.machine_id, m.name AS machine_name \
			 FROM applications a JOIN machines m ON m.id = a.machine_id \
			 WHERE a.id <> '00000000-0000-0000-0000-000000000000' \
			 ORDER BY a.name",
		)
		.load(&mut conn)
		.await
		.expect("the migrated applications");
		assert_eq!(pairs.len(), 2, "one application per server, and no more");
		assert_eq!(pairs[0].name, "box-one");
		assert_eq!(pairs[0].type_, "tamanu-central");
		assert_eq!(pairs[0].machine_name.as_deref(), Some("box-one"));
		assert_eq!(pairs[1].type_, "tamanu-facility");

		let machines: Count =
			diesel::sql_query("SELECT COUNT(*) AS n FROM machines WHERE id <> $1")
				.bind::<sql_types::Uuid, _>(Uuid::parse_str(NIL).unwrap())
				.get_result(&mut conn)
				.await
				.expect("count machines");
		assert_eq!(machines.n, 2, "one box per server, not a shared one");
		assert_ne!(
			pairs[0].machine_id, pairs[1].machine_id,
			"two servers are two boxes"
		);

		// The box's own facts are the box's.
		#[derive(diesel::QueryableByName)]
		struct MachineFacts {
			#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
			group_id: Option<Uuid>,
			#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
			device_id: Option<Uuid>,
			#[diesel(sql_type = sql_types::Text)]
			alert_when_down_for: String,
			#[diesel(sql_type = sql_types::Nullable<sql_types::Bool>)]
			cloud: Option<bool>,
		}
		let facts: MachineFacts = diesel::sql_query(
			"SELECT group_id, device_id, alert_when_down_for::text AS alert_when_down_for, cloud \
			 FROM machines WHERE name = 'box-one'",
		)
		.get_result(&mut conn)
		.await
		.expect("box-one's machine");
		assert_eq!(
			facts.group_id,
			Some(Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()),
			"which deployment a box belongs to is the box's"
		);
		assert_eq!(
			facts.device_id,
			Some(Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()),
			"the identity speaks for the box"
		);
		assert_eq!(facts.alert_when_down_for, "00:17:00");
		assert_eq!(facts.cloud, Some(true));

		// The identity's role names what it authenticates, and that is a box.
		let role: Count = diesel::sql_query(
			"SELECT COUNT(*) AS n FROM devices WHERE role = 'machine' \
			 AND id = '22222222-2222-2222-2222-222222222222'",
		)
		.get_result(&mut conn)
		.await
		.expect("the identity");
		assert_eq!(role.n, 1, "the identity's role is a machine's");

		// Nothing that named the server was dropped or moved off it.
		#[derive(diesel::QueryableByName)]
		struct Scoped {
			#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
			application_id: Option<Uuid>,
		}
		let box_one = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();

		let issue: Scoped = diesel::sql_query(
			"SELECT application_id FROM issues \
			 WHERE id = '55555555-5555-5555-5555-555555555555' AND active",
		)
		.get_result(&mut conn)
		.await
		.expect("the check state survives");
		assert_eq!(issue.application_id, Some(box_one));

		let policies: Vec<Scoped> = diesel::sql_query(
			"SELECT application_id FROM scoped_check_policies \
			 WHERE check_name = 'db' AND ceiling = 'warning'",
		)
		.load(&mut conn)
		.await
		.expect("the silence survives");
		assert_eq!(policies.len(), 1, "one application is one policy row");
		assert_eq!(policies[0].application_id, Some(box_one));

		let linked: Count = diesel::sql_query(
			"SELECT COUNT(*) AS n FROM incident_issues \
			 WHERE incident_id = '77777777-7777-7777-7777-777777777777' \
			   AND issue_id = '55555555-5555-5555-5555-555555555555'",
		)
		.get_result(&mut conn)
		.await
		.expect("the incident survives");
		assert_eq!(linked.n, 1, "the incident still holds its check state");
	})
	.await;
}

/// A migrated application has no key, because nothing reported it under one.
/// That is what lets the first split push naming its type take it over.
// spec: APP, STA
#[tokio::test(flavor = "multi_thread")]
async fn a_migrated_application_carries_no_reporter_key() {
	commons_tests::db::TestDb::run(async |mut conn, url| {
		revert_the_split(&url).await;

		conn.batch_execute(
			"INSERT INTO servers (name, host, product, kind) \
			 VALUES ('keyless', 'https://keyless.invalid/', 'tamanu', 'central')",
		)
		.await
		.expect("seed a server");

		apply_the_split(&url).await;

		let keyed: Count = diesel::sql_query(
			"SELECT COUNT(*) AS n FROM applications \
			 WHERE id <> $1 AND reported_key IS NOT NULL",
		)
		.bind::<sql_types::Uuid, _>(Uuid::parse_str(NIL).unwrap())
		.get_result(&mut conn)
		.await
		.expect("count the keyed applications");
		assert_eq!(
			keyed.n, 0,
			"nothing reported the migrated application under a key"
		);
	})
	.await;
}

/// A body that is not an object becomes the empty object, and does not stop the
/// run.
///
/// `extra` is `JSONB NOT NULL`, which admits JSON `null` and every other
/// scalar, and nothing constrained it before the split. Such rows existed in
/// the field and failed a production deploy: `jsonb_each` refuses a non-object,
/// and so does the `-` key delete on the statement after it. The split
/// therefore flattens a non-object first — it carries no fields, so the empty
/// object says the same thing and every reader can walk it, which a preserved
/// scalar could not.
// spec: FIG
#[tokio::test(flavor = "multi_thread")]
async fn detail_that_is_not_an_object_becomes_an_empty_object() {
	commons_tests::db::TestDb::run(async |mut conn, url| {
		revert_the_split(&url).await;

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, product, kind) \
			 VALUES ('88888888-8888-8888-8888-888888888888', 'odd-box', \
			         'https://odd.invalid/', 'tamanu', 'central');

			 INSERT INTO server_reported_detail (server_id, source, extra) \
			 VALUES \
			   ('88888888-8888-8888-8888-888888888888', 'alertd', \
			    '{\"osName\": \"Ubuntu\", \"pgVersion\": \"16.3\"}'::jsonb), \
			   ('88888888-8888-8888-8888-888888888888', 'scalar', \
			    '\"not an object\"'::jsonb), \
			   ('88888888-8888-8888-8888-888888888888', 'listy', \
			    '[1, 2, 3]'::jsonb), \
			   ('88888888-8888-8888-8888-888888888888', 'nully', \
			    'null'::jsonb)",
		)
		.await
		.expect("seed detail of assorted shapes");

		apply_the_split(&url).await;

		#[derive(Debug, diesel::QueryableByName)]
		struct Detail {
			#[diesel(sql_type = sql_types::Text)]
			source: String,
			#[diesel(sql_type = sql_types::Jsonb)]
			extra: serde_json::Value,
		}

		// Only the object row had a field belonging to the box.
		let machine: Vec<Detail> =
			diesel::sql_query("SELECT source, extra FROM machine_reported_detail ORDER BY source")
				.load(&mut conn)
				.await
				.expect("the box's detail");
		assert_eq!(
			machine.len(),
			1,
			"a non-object row has no field to give the box: {machine:?}",
		);
		assert_eq!(machine[0].source, "alertd");
		assert_eq!(machine[0].extra, serde_json::json!({"osName": "Ubuntu"}));

		let application: Vec<Detail> = diesel::sql_query(
			"SELECT source, extra FROM application_reported_detail ORDER BY source",
		)
		.load(&mut conn)
		.await
		.expect("the workload's detail");
		let by_source: std::collections::HashMap<&str, &serde_json::Value> = application
			.iter()
			.map(|d| (d.source.as_str(), &d.extra))
			.collect();

		assert_eq!(
			by_source.get("alertd"),
			Some(&&serde_json::json!({"pgVersion": "16.3"})),
			"the box's field left the workload's row and the workload's own stayed",
		);
		for source in ["scalar", "listy", "nully"] {
			assert_eq!(
				by_source.get(source),
				Some(&&serde_json::json!({})),
				"{source} was flattened rather than kept as a body no reader can walk",
			);
		}

		// The operation the deploy died on, over every row the split produced.
		let walked: Count = diesel::sql_query(
			"SELECT count(*) AS n FROM ( \
			   SELECT jsonb_each(extra) FROM application_reported_detail \
			   UNION ALL SELECT jsonb_each(extra) FROM machine_reported_detail \
			 ) AS walked",
		)
		.get_result(&mut conn)
		.await
		.expect("jsonb_each walks every body the split produced");
		assert_eq!(
			walked.n, 2,
			"the two fields of the one object row, and nothing from the rest",
		);
	})
	.await;
}
