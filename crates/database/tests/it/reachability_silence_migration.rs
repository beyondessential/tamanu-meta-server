//! The `2026-09-03-111344-0000_reachability_silences_take_the_machine`
//! migration carries an unreachability silence from an application onto the box
//! it runs on.
//!
//! Reachability is filed at both grains now. An operator who silenced it before
//! the split silenced the only filing there was; after it, the box files its own
//! and every deliberately quiet host opens an incident. The migration carries
//! the instruction across rather than making the operator restate it.
//!
//! Runs the migration's SQL for real. It is additive against the current
//! schema, so there is nothing to revert first — the pre-state is simply an
//! application-scoped silence.

use diesel::sql_types;
use diesel_async::{RunQueryDsl, SimpleAsyncConnection as _};
use uuid::Uuid;

const UP: &str = include_str!(
	"../../../../migrations/2026-09-03-111344-0000_reachability_silences_take_the_machine/up.sql"
);
const DOWN: &str = include_str!(
	"../../../../migrations/2026-09-03-111344-0000_reachability_silences_take_the_machine/down.sql"
);

#[derive(diesel::QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

#[derive(diesel::QueryableByName)]
struct Count {
	#[diesel(sql_type = sql_types::BigInt)]
	n: i64,
}

#[derive(diesel::QueryableByName)]
struct Silence {
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	created_by: Option<String>,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	ceiling: Option<String>,
}

async fn machine(conn: &mut diesel_async::AsyncPgConnection, group: Uuid) -> Uuid {
	let row: RowId =
		diesel::sql_query("INSERT INTO machines (group_id, name) VALUES ($1, 'box') RETURNING id")
			.bind::<sql_types::Uuid, _>(group)
			.get_result(conn)
			.await
			.expect("machine");
	row.id
}

async fn application(conn: &mut diesel_async::AsyncPgConnection, machine_id: Uuid) -> Uuid {
	let row: RowId = diesel::sql_query(
		"INSERT INTO applications (host, type, machine_id) \
		 VALUES ('http://sil.invalid/', 'tamanu-central', $1) RETURNING id",
	)
	.bind::<sql_types::Uuid, _>(machine_id)
	.get_result(conn)
	.await
	.expect("application");
	row.id
}

async fn group(conn: &mut diesel_async::AsyncPgConnection) -> Uuid {
	let row: RowId =
		diesel::sql_query("INSERT INTO server_groups (name) VALUES ('g') RETURNING id")
			.get_result(conn)
			.await
			.expect("group");
	row.id
}

async fn silence_application(
	conn: &mut diesel_async::AsyncPgConnection,
	application_id: Uuid,
	by: &str,
) {
	diesel::sql_query(
		"INSERT INTO scoped_check_policies (source, check_name, application_id, ceiling, created_by) \
		 VALUES ('canopy', 'reachability', $1, 'skipped', $2)",
	)
	.bind::<sql_types::Uuid, _>(application_id)
	.bind::<sql_types::Text, _>(by)
	.execute(conn)
	.await
	.expect("silence the application");
}

async fn machine_silences(
	conn: &mut diesel_async::AsyncPgConnection,
	machine_id: Uuid,
) -> Vec<Silence> {
	diesel::sql_query(
		"SELECT created_by, ceiling FROM scoped_check_policies \
		 WHERE machine_id = $1 AND source = 'canopy' AND check_name = 'reachability'",
	)
	.bind::<sql_types::Uuid, _>(machine_id)
	.load(conn)
	.await
	.expect("read machine silences")
}

// spec: CHK#reachability
#[tokio::test(flavor = "multi_thread")]
async fn a_silenced_application_silences_its_box() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let g = group(&mut conn).await;
		let m = machine(&mut conn, g).await;
		let app = application(&mut conn, m).await;
		silence_application(&mut conn, app, "alice").await;

		conn.batch_execute(UP).await.expect("apply");

		let carried = machine_silences(&mut conn, m).await;
		assert_eq!(carried.len(), 1, "the box takes the silence");
		assert_eq!(
			carried[0].ceiling.as_deref(),
			Some("skipped"),
			"carried as a silence, not some other transform"
		);
		assert_eq!(
			carried[0].created_by.as_deref(),
			Some("alice"),
			"the instruction stays attributed to whoever gave it"
		);
	})
	.await
}

/// Silencing one of two workloads was never a statement about the host, so the
/// box is left alerting. Over-silencing here would quietly stop paging for a
/// box the operator never asked about.
// spec: CHK#reachability
#[tokio::test(flavor = "multi_thread")]
async fn a_partly_silenced_box_is_left_alone() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let g = group(&mut conn).await;
		let m = machine(&mut conn, g).await;
		let quiet = application(&mut conn, m).await;
		let _loud = application(&mut conn, m).await;
		silence_application(&mut conn, quiet, "alice").await;

		conn.batch_execute(UP).await.expect("apply");

		assert!(
			machine_silences(&mut conn, m).await.is_empty(),
			"one of two workloads silenced says nothing about the box"
		);
	})
	.await
}

/// A box with nothing live on it has no instruction to derive.
// spec: CHK#reachability
#[tokio::test(flavor = "multi_thread")]
async fn a_bare_box_is_left_alone() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let g = group(&mut conn).await;
		let m = machine(&mut conn, g).await;

		conn.batch_execute(UP).await.expect("apply");

		assert!(
			machine_silences(&mut conn, m).await.is_empty(),
			"a box awaiting its first report is not silenced"
		);
	})
	.await
}

/// Re-running it must not double up: migrations are replayed in tests and
/// re-applied by hand, and a second silence row on one box would collide with
/// the uniqueness the schema holds over a scope's transforms.
// spec: CHK#reachability
#[tokio::test(flavor = "multi_thread")]
async fn applying_it_twice_changes_nothing() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let g = group(&mut conn).await;
		let m = machine(&mut conn, g).await;
		let app = application(&mut conn, m).await;
		silence_application(&mut conn, app, "alice").await;

		conn.batch_execute(UP).await.expect("apply");
		conn.batch_execute(UP).await.expect("apply again");

		assert_eq!(machine_silences(&mut conn, m).await.len(), 1);
	})
	.await
}

/// An operator who has already silenced the box keeps their own row, rather
/// than having the migration write a second one over the top of it.
// spec: CHK#reachability
#[tokio::test(flavor = "multi_thread")]
async fn an_existing_machine_silence_stands() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let g = group(&mut conn).await;
		let m = machine(&mut conn, g).await;
		let app = application(&mut conn, m).await;
		silence_application(&mut conn, app, "alice").await;
		diesel::sql_query(
			"INSERT INTO scoped_check_policies (source, check_name, machine_id, ceiling, created_by) \
			 VALUES ('canopy', 'reachability', $1, 'skipped', 'bob')",
		)
		.bind::<sql_types::Uuid, _>(m)
		.execute(&mut conn)
		.await
		.expect("operator silence");

		conn.batch_execute(UP).await.expect("apply");

		let held = machine_silences(&mut conn, m).await;
		assert_eq!(held.len(), 1);
		assert_eq!(held[0].created_by.as_deref(), Some("bob"));
	})
	.await
}

// spec: CHK#reachability
#[tokio::test(flavor = "multi_thread")]
async fn reverting_takes_the_carried_silence_back_off() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let g = group(&mut conn).await;
		let m = machine(&mut conn, g).await;
		let app = application(&mut conn, m).await;
		silence_application(&mut conn, app, "alice").await;

		conn.batch_execute(UP).await.expect("apply");
		conn.batch_execute(DOWN).await.expect("revert");

		assert!(machine_silences(&mut conn, m).await.is_empty());

		// The application's own silence is untouched: it was never this
		// migration's to move.
		let remaining: Count = diesel::sql_query(
			"SELECT count(*) AS n FROM scoped_check_policies \
			 WHERE application_id = $1 AND source = 'canopy' AND check_name = 'reachability'",
		)
		.bind::<sql_types::Uuid, _>(app)
		.get_result(&mut conn)
		.await
		.expect("count");
		assert_eq!(remaining.n, 1);
	})
	.await
}
