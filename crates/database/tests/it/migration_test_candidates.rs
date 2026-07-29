//! Candidate derivation: which versions a server is asked to be tested
//! against. One per minor series, newest patch, within the server's major,
//! and only for Tamanu servers that have reported a version.

use commons_types::{
	server::product::Product,
	version::{VersionStatus, VersionStr},
};
use database::{
	migration_tests::{Candidate, candidates, upgrade_path},
	reported_detail::ReportedDetail,
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

fn reported(text: &str) -> VersionStr {
	text.parse().expect("parse reported version")
}

async fn add_version(
	conn: &mut diesel_async::AsyncPgConnection,
	major: i32,
	minor: i32,
	patch: i32,
	status: VersionStatus,
) -> Version {
	diesel::insert_into(database::schema::versions::table)
		.values(NewVersion {
			major,
			minor,
			patch,
			status,
			changelog: String::new(),
			device_id: None,
		})
		.returning(Version::as_returning())
		.get_result(conn)
		.await
		.expect("insert version")
}

async fn publish(
	conn: &mut diesel_async::AsyncPgConnection,
	major: i32,
	minor: i32,
	patch: i32,
) -> Version {
	add_version(conn, major, minor, patch, VersionStatus::Published).await
}

async fn insert_server(
	conn: &mut diesel_async::AsyncPgConnection,
	host: &str,
	product: Product,
) -> Uuid {
	let group: RowId = sql_query("INSERT INTO server_groups (name) VALUES ('kamaka') RETURNING id")
		.get_result(conn)
		.await
		.expect("group");
	let server: RowId =
		sql_query("INSERT INTO servers (host, group_id, product) VALUES ($1, $2, $3) RETURNING id")
			.bind::<sql_types::Text, _>(host)
			.bind::<sql_types::Uuid, _>(group.id)
			.bind::<sql_types::Text, _>(product.to_string())
			.get_result(conn)
			.await
			.expect("server");
	server.id
}

#[tokio::test(flavor = "multi_thread")]
async fn newest_patch_of_each_minor_ahead_of_the_server() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let behind = publish(&mut conn, 2, 61, 0).await;
		let same_minor_older = publish(&mut conn, 2, 62, 1).await;
		let same_minor_newest = publish(&mut conn, 2, 62, 4).await;
		let next_minor_older = publish(&mut conn, 2, 63, 0).await;
		let next_minor_newest = publish(&mut conn, 2, 63, 2).await;
		let other_major = publish(&mut conn, 3, 0, 0).await;

		let versions = Version::get_all(&mut conn).await.expect("versions");
		let path = upgrade_path(&reported("2.62.0"), &versions);

		assert_eq!(
			path,
			vec![same_minor_newest.id, next_minor_newest.id],
			"one entry per minor, each the newest patch"
		);
		assert!(!path.contains(&behind.id), "already past it");
		assert!(!path.contains(&same_minor_older.id), "superseded patch");
		assert!(!path.contains(&next_minor_older.id), "superseded patch");
		assert!(
			!path.contains(&other_major.id),
			"the path stays within the major"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_on_the_newest_version_has_no_candidates() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		publish(&mut conn, 2, 62, 0).await;
		let versions = Version::get_all(&mut conn).await.expect("versions");

		assert!(upgrade_path(&reported("2.62.0"), &versions).is_empty());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn drafts_are_not_candidates_on_their_own() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let draft = add_version(&mut conn, 2, 63, 0, VersionStatus::Draft).await;

		let all = Version::get_all_including_drafts(&mut conn)
			.await
			.expect("versions");
		assert!(
			!upgrade_path(&reported("2.62.0"), &all).contains(&draft.id),
			"an unpublished version reaches a server only by nomination"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn only_tamanu_servers_that_reported_a_version() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let target = publish(&mut conn, 2, 63, 0).await;

		let tamanu =
			insert_server(&mut conn, "https://central.kamaka.example", Product::Tamanu).await;
		let senaite =
			insert_server(&mut conn, "https://lims.kamaka.example", Product::Senaite).await;
		let silent =
			insert_server(&mut conn, "https://quiet.kamaka.example", Product::Tamanu).await;

		let running = reported("2.62.0");
		for server in [tamanu, senaite] {
			ReportedDetail::record(
				&mut conn,
				server,
				"test",
				&serde_json::json!({}),
				Some(&running),
			)
			.await
			.expect("record detail");
		}

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
		assert!(
			!found.iter().any(|c| c.server_id == silent),
			"nothing to compare against without a reported version"
		);
	})
	.await
}
