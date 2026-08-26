//! The unified `Scope` type: storage-column mapping and incident-target
//! resolution — the single place check-state scope is interpreted.

use database::issues::{IncidentTarget, Scope};
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[test]
fn columns_round_trip() {
	let s = Uuid::new_v4();
	let g = Uuid::new_v4();
	assert_eq!(Scope::Application(s).to_columns(), (Some(s), None));
	assert_eq!(Scope::Group(g).to_columns(), (None, Some(g)));
	assert_eq!(Scope::Global.to_columns(), (None, None));
	assert_eq!(Scope::from_columns(Some(s), None), Scope::Application(s));
	assert_eq!(Scope::from_columns(None, Some(g)), Scope::Group(g));
	assert_eq!(Scope::from_columns(None, None), Scope::Global);
	// The storage CHECK forbids both being set; if it ever happens, a set
	// group wins (matches the historical scope-resolution order).
	assert_eq!(Scope::from_columns(Some(s), Some(g)), Scope::Group(g));
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
	let row: RowId = sql_query(
		"INSERT INTO applications (host, group_id, is_monitored) VALUES ($1, $2, $3) RETURNING id",
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group)
	.bind::<sql_types::Bool, _>(monitored)
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
