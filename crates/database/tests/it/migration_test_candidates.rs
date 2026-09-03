//! Candidate derivation: which version a server is asked to be tested against.
//! The version its group's open plan names, and only for Tamanu servers.

use commons_tests::db::TestDb;
use commons_types::{
	server::{product::Product, rank::ServerRank},
	version::VersionStatus,
};
use database::{
	migration_tests::{Candidate, candidates},
	upgrade_plans::{PlannedWhen, UpgradePlan},
	versions::{NewVersion, Version},
};
use diesel::{QueryableByName, SelectableHelper, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn publish(
	conn: &mut diesel_async::AsyncPgConnection,
	major: i32,
	minor: i32,
	patch: i32,
) -> Version {
	diesel::insert_into(database::schema::versions::table)
		.values(NewVersion {
			major,
			minor,
			patch,
			status: VersionStatus::Published,
			changelog: String::new(),
			device_id: None,
		})
		.returning(Version::as_returning())
		.get_result(conn)
		.await
		.expect("insert version")
}

async fn insert_group(conn: &mut diesel_async::AsyncPgConnection, name: &str) -> Uuid {
	let group: RowId = sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(name)
		.get_result(conn)
		.await
		.expect("group");
	group.id
}

async fn insert_server(
	conn: &mut diesel_async::AsyncPgConnection,
	group: Uuid,
	host: &str,
	product: Product,
) -> Uuid {
	let server: RowId =
		sql_query(
			"INSERT INTO servers (host, rank, group_id, product) VALUES ($1, 'production', $2, $3) RETURNING id",
		)
			.bind::<sql_types::Text, _>(host)
			.bind::<sql_types::Uuid, _>(group)
			.bind::<sql_types::Text, _>(product.to_string())
			.get_result(conn)
			.await
			.expect("server");
	server.id
}

async fn plan(conn: &mut diesel_async::AsyncPgConnection, group: Uuid, target: &Version) {
	UpgradePlan::record(
		conn,
		group,
		ServerRank::Production,
		target.id,
		PlannedWhen::default(),
		None,
		"someone@example.com",
	)
	.await
	.expect("record plan");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_server_in_a_planned_group_is_a_candidate() {
	TestDb::run(|mut conn, _url| async move {
		let target = publish(&mut conn, 2, 63, 0).await;
		let group = insert_group(&mut conn, "kamaka").await;
		let central = insert_server(
			&mut conn,
			group,
			"https://central.kamaka.example",
			Product::Tamanu,
		)
		.await;
		let facility = insert_server(
			&mut conn,
			group,
			"https://facility.kamaka.example",
			Product::Tamanu,
		)
		.await;
		plan(&mut conn, group, &target).await;

		let mut found = candidates(&mut conn).await.expect("candidates");
		found.sort_by_key(|c| c.server_id);
		let mut want = vec![
			Candidate {
				server_id: central,
				version_id: target.id,
			},
			Candidate {
				server_id: facility,
				version_id: target.id,
			},
		];
		want.sort_by_key(|c| c.server_id);

		assert_eq!(found, want, "the plan covers the whole group");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_group_with_no_plan_has_no_candidates() {
	TestDb::run(|mut conn, _url| async move {
		publish(&mut conn, 2, 63, 0).await;
		let group = insert_group(&mut conn, "drifting").await;
		insert_server(
			&mut conn,
			group,
			"https://central.drifting.example",
			Product::Tamanu,
		)
		.await;

		assert!(
			candidates(&mut conn).await.expect("candidates").is_empty(),
			"a restore costs hours, and nobody has said this group is moving"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_plan_stops_the_testing() {
	TestDb::run(|mut conn, _url| async move {
		let target = publish(&mut conn, 2, 63, 0).await;
		let group = insert_group(&mut conn, "kamaka").await;
		insert_server(
			&mut conn,
			group,
			"https://central.kamaka.example",
			Product::Tamanu,
		)
		.await;
		plan(&mut conn, group, &target).await;

		let open = UpgradePlan::open_for_environment(&mut conn, group, ServerRank::Production)
			.await
			.expect("open plan")
			.expect("a plan is open");
		UpgradePlan::withdraw(&mut conn, open.id, "someone@example.com")
			.await
			.expect("withdraw");

		assert!(
			candidates(&mut conn).await.expect("candidates").is_empty(),
			"the group stopped going there, so there is nothing to hold its data against"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn only_tamanu_servers() {
	TestDb::run(|mut conn, _url| async move {
		let target = publish(&mut conn, 2, 63, 0).await;
		let group = insert_group(&mut conn, "kamaka").await;
		let tamanu = insert_server(
			&mut conn,
			group,
			"https://central.kamaka.example",
			Product::Tamanu,
		)
		.await;
		let senaite = insert_server(
			&mut conn,
			group,
			"https://lims.kamaka.example",
			Product::Senaite,
		)
		.await;
		plan(&mut conn, group, &target).await;

		let found = candidates(&mut conn).await.expect("candidates");

		assert_eq!(
			found,
			vec![Candidate {
				server_id: tamanu,
				version_id: target.id,
			}],
		);
		assert!(
			!found.iter().any(|c| c.server_id == senaite),
			"another product has no path through Tamanu's migrations"
		);
	})
	.await
}
