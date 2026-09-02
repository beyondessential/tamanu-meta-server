//! The unified `Scope` type: storage-column mapping and incident-target
//! resolution — the single place check-state scope is interpreted.

use database::issues::{IncidentTarget, Scope};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[test]
fn columns_round_trip() {
	let s = Uuid::new_v4();
	let m = Uuid::new_v4();
	let g = Uuid::new_v4();
	assert_eq!(Scope::Application(s).to_columns(), (Some(s), None, None));
	assert_eq!(Scope::Machine(m).to_columns(), (None, Some(m), None));
	assert_eq!(Scope::Group(g).to_columns(), (None, None, Some(g)));
	assert_eq!(Scope::Global.to_columns(), (None, None, None));
	assert_eq!(
		Scope::from_columns(Some(s), None, None),
		Scope::Application(s)
	);
	assert_eq!(Scope::from_columns(None, Some(m), None), Scope::Machine(m));
	assert_eq!(Scope::from_columns(None, None, Some(g)), Scope::Group(g));
	assert_eq!(Scope::from_columns(None, None, None), Scope::Global);
	// The storage CHECK forbids more than one being set; if it ever happens, a
	// set group wins (matches the historical scope-resolution order), then a
	// machine.
	assert_eq!(Scope::from_columns(Some(s), None, Some(g)), Scope::Group(g));
	assert_eq!(
		Scope::from_columns(Some(s), Some(m), None),
		Scope::Machine(m)
	);
}

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_group(conn: &mut diesel_async::AsyncPgConnection) -> Uuid {
	let row: RowId = sql_query("INSERT INTO server_groups (name) VALUES ('g') RETURNING id")
		.get_result(conn)
		.await
		.expect("insert group");
	row.id
}

async fn insert_server(
	conn: &mut diesel_async::AsyncPgConnection,
	host: &str,
	group: Option<Uuid>,
	monitored: bool,
) -> Uuid {
	let machine: RowId = sql_query("INSERT INTO machines (group_id) VALUES ($1) RETURNING id")
		.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group)
		.get_result(conn)
		.await
		.expect("insert machine");
	let row: RowId = sql_query(
		"INSERT INTO applications (type, host, group_id, is_monitored, machine_id) VALUES ('tamanu-central', $1, $2, $3, $4) RETURNING id",
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group)
	.bind::<sql_types::Bool, _>(monitored)
	.bind::<sql_types::Uuid, _>(machine.id)
	.get_result(conn)
	.await
	.expect("insert server");
	row.id
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_incident_target_by_scope() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn).await;
		let monitored = insert_server(&mut conn, "http://mon.invalid/", Some(group), true).await;
		let unmonitored =
			insert_server(&mut conn, "http://unmon.invalid/", Some(group), false).await;
		let ungrouped = insert_server(&mut conn, "http://ungrouped.invalid/", None, true).await;

		// A server in a group targets its group, carrying its is_monitored.
		assert_eq!(
			Scope::Application(monitored)
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			Some((IncidentTarget::Group(group), true)),
		);
		assert_eq!(
			Scope::Application(unmonitored)
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			Some((IncidentTarget::Group(group), false)),
		);
		// An ungrouped server has no target and no incident path.
		assert_eq!(
			Scope::Application(ungrouped)
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			None,
		);
		// Group and canopy-wide scopes target themselves, always monitored.
		assert_eq!(
			Scope::Group(group)
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			Some((IncidentTarget::Group(group), true)),
		);
		assert_eq!(
			Scope::Global
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			Some((IncidentTarget::Global, true)),
		);
	})
	.await
}

async fn insert_machine(
	conn: &mut diesel_async::AsyncPgConnection,
	group: Option<Uuid>,
	monitored: bool,
) -> Uuid {
	let row: RowId =
		sql_query("INSERT INTO machines (group_id, is_monitored) VALUES ($1, $2) RETURNING id")
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group)
			.bind::<sql_types::Bool, _>(monitored)
			.get_result(conn)
			.await
			.expect("insert machine");
	row.id
}

/// A machine resolves through its group like an application does, but carries
/// its own monitoring switch — so excusing a box from monitoring says nothing
/// about the workloads on it.
// spec: CHK
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_resolves_through_its_group_on_its_own_switch() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn).await;
		let monitored = insert_machine(&mut conn, Some(group), true).await;
		let unmonitored = insert_machine(&mut conn, Some(group), false).await;
		let ungrouped = insert_machine(&mut conn, None, true).await;

		assert_eq!(
			Scope::Machine(monitored)
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			Some((IncidentTarget::Group(group), true)),
		);
		assert_eq!(
			Scope::Machine(unmonitored)
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			Some((IncidentTarget::Group(group), false)),
		);
		// An ungrouped machine has no target, exactly as an ungrouped
		// application has none.
		assert_eq!(
			Scope::Machine(ungrouped)
				.resolve_incident_target(&mut conn)
				.await
				.expect("resolve"),
			None,
		);
	})
	.await
}

/// The trap the machine grain had to dodge: the global-scope partial unique
/// index matches on every *other* scope column being null, so a machine-scoped
/// row would fall inside it and collide with a canopy-wide issue on the same
/// `(source, ref)` unless the index excludes machines too.
// spec: CHK
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_issue_does_not_collide_with_a_canopy_wide_one() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let machine = insert_machine(&mut conn, None, true).await;

		database::issues::raise_global_event(
			&mut conn,
			"disk_free",
			None,
			"canopy's own disk",
			true,
		)
		.await
		.expect("canopy-wide issue");

		// Same (source, ref), different grain: this must be its own row.
		database::issues::raise_machine_event_with_state(
			&mut conn,
			machine,
			"canopy",
			None,
			"disk_free",
			None,
			"the box's disk",
			true,
			None,
		)
		.await
		.expect("a machine check must not collide with a self-alert");

		let rows: Vec<RowId> =
			sql_query("SELECT id FROM issues WHERE source = 'canopy' AND ref = 'disk_free'")
				.load(&mut conn)
				.await
				.expect("load");
		assert_eq!(rows.len(), 2, "one issue per grain, not one shared row");
	})
	.await
}

/// A degraded machine check is one issue at machine scope however many
/// applications run on the box — the whole point of the grain.
// spec: CHK
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_check_files_once_not_once_per_application() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn).await;
		let machine = insert_machine(&mut conn, Some(group), true).await;

		for host in ["http://one.invalid/", "http://two.invalid/"] {
			sql_query("INSERT INTO applications (type, host, group_id, machine_id) VALUES ('tamanu-central', $1, $2, $3)")
				.bind::<sql_types::Text, _>(host)
				.bind::<sql_types::Uuid, _>(group)
				.bind::<sql_types::Uuid, _>(machine)
				.execute(&mut conn)
				.await
				.expect("insert application");
		}

		// Filed twice: find-or-create keys on the machine, so it coalesces.
		for message in ["disk 9% free", "disk 4% free"] {
			database::issues::raise_machine_event_with_state(
				&mut conn,
				machine,
				"canopy",
				None,
				"disk_free",
				None,
				message,
				true,
				None,
			)
			.await
			.expect("file machine check");
		}

		let rows: Vec<RowId> =
			sql_query("SELECT id FROM issues WHERE machine_id = $1 AND ref = 'disk_free'")
				.bind::<sql_types::Uuid, _>(machine)
				.load(&mut conn)
				.await
				.expect("load");
		assert_eq!(
			rows.len(),
			1,
			"one issue for the box, not one per workload on it"
		);

		let on_apps: Vec<RowId> = sql_query(
			"SELECT id FROM issues WHERE application_id IS NOT NULL AND ref = 'disk_free'",
		)
		.load(&mut conn)
		.await
		.expect("load");
		assert!(
			on_apps.is_empty(),
			"a machine's check is not attributed to any application"
		);
	})
	.await
}
