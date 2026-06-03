//! Group archival (soft-delete): a group can be archived only when it has no
//! *live* members; archived groups drop out of live listings and show up in the
//! archived listing; restore reverses it. Also covers `Server::list_archived`.

use commons_tests::db::TestDb;
use database::server_groups::ServerGroup;
use database::servers::Server;
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

/// Insert a server in a group; `archived` sets `deleted_at`.
async fn insert_server(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group_id: Uuid,
	archived: bool,
) -> Uuid {
	#[derive(diesel::QueryableByName)]
	struct RowId {
		#[diesel(sql_type = sql_types::Uuid)]
		id: Uuid,
	}
	let host = format!("http://test.invalid/{}", Uuid::new_v4());
	let row: RowId = sql_query(
		"INSERT INTO servers (host, kind, group_id, deleted_at)
		 VALUES ($1, 'central', $2, CASE WHEN $3 THEN now() ELSE NULL END) RETURNING id",
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Bool, _>(archived)
	.get_result(conn)
	.await
	.expect("insert server");
	row.id
}

#[tokio::test(flavor = "multi_thread")]
async fn group_archival_round_trips_and_guards_live_members() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "Group").await;

		// A live member blocks archival.
		let live = insert_server(&mut conn, group, false).await;
		assert!(
			ServerGroup::soft_delete(&mut conn, group).await.is_err(),
			"a group with a live member can't be archived",
		);

		// Archive that member; now the group has only-archived members → allowed.
		Server::soft_delete(&mut conn, live).await.unwrap();
		ServerGroup::soft_delete(&mut conn, group).await.unwrap();

		// (a) live listings exclude it; (b) archived listing includes it.
		let live_ids: Vec<Uuid> = ServerGroup::list_all(&mut conn)
			.await
			.unwrap()
			.into_iter()
			.map(|g| g.id)
			.collect();
		assert!(!live_ids.contains(&group), "archived group hidden from list_all");
		let archived_ids: Vec<Uuid> = ServerGroup::list_archived(&mut conn)
			.await
			.unwrap()
			.into_iter()
			.map(|g| g.id)
			.collect();
		assert!(archived_ids.contains(&group), "archived group shows in list_archived");

		// get_by_id still finds it (detail page needs it to offer Restore).
		assert!(
			ServerGroup::get_by_id(&mut conn, group)
				.await
				.unwrap()
				.deleted_at
				.is_some(),
		);

		// Restore: back in live listings, gone from archived.
		ServerGroup::restore(&mut conn, group).await.unwrap();
		let live_ids: Vec<Uuid> = ServerGroup::list_all(&mut conn)
			.await
			.unwrap()
			.into_iter()
			.map(|g| g.id)
			.collect();
		assert!(live_ids.contains(&group), "restored group back in list_all");
		assert!(
			ServerGroup::list_archived(&mut conn).await.unwrap().is_empty(),
			"nothing archived after restore",
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_list_archived_only_returns_archived() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "G").await;
		let live = insert_server(&mut conn, group, false).await;
		let archived = insert_server(&mut conn, group, true).await;

		let archived_ids: Vec<Uuid> = Server::list_archived(&mut conn)
			.await
			.unwrap()
			.into_iter()
			.map(|s| s.id)
			.collect();
		assert!(archived_ids.contains(&archived), "archived server listed");
		assert!(!archived_ids.contains(&live), "live server not in archived list");

		let live_ids: Vec<Uuid> = Server::get_all(&mut conn, 0, None)
			.await
			.unwrap()
			.into_iter()
			.map(|s| s.id)
			.collect();
		assert!(live_ids.contains(&live), "live server in get_all");
		assert!(!live_ids.contains(&archived), "archived server excluded from get_all");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn live_server_counts_excludes_archived() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "G").await;
		insert_server(&mut conn, group, false).await; // live
		insert_server(&mut conn, group, false).await; // live
		insert_server(&mut conn, group, true).await; // archived — excluded

		let counts = ServerGroup::live_server_counts(&mut conn).await.unwrap();
		assert_eq!(
			counts.get(&group).copied(),
			Some(2),
			"counts only live (non-archived) members",
		);
	})
	.await
}
