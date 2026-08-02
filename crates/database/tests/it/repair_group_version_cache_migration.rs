//! The `2026-08-02-011917-0000_repair_group_version_cache` migration
//! re-derives each group's canonical member, undoing what the original
//! backfill in `2026-06-02-071412` got wrong: its rank `CASE` listed only the
//! canonical spellings (so `live` / `prod` / `staging` fell to the
//! lowest-priority bucket), and it selected among archived servers too.
//!
//! Seeds the states that backfill mis-handled, writes the cache it would have
//! left, and replays the repair's SQL.

use commons_tests::db::TestDb;
use diesel::{sql_query, sql_types};
use diesel_async::{RunQueryDsl, SimpleAsyncConnection as _};
use uuid::Uuid;

const REPAIR_UP: &str =
	include_str!("../../../../migrations/2026-08-02-011917-0000_repair_group_version_cache/up.sql");

#[derive(diesel::QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

async fn insert_group(conn: &mut diesel_async::AsyncPgConnection, name: &str) -> Uuid {
	let row: RowId = sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(name)
		.get_result(conn)
		.await
		.expect("insert group");
	row.id
}

/// A member with a raw `rank` string — the point is to seed spellings the
/// original backfill didn't recognise, so this deliberately doesn't go
/// through `ServerRank`.
async fn insert_member(
	conn: &mut diesel_async::AsyncPgConnection,
	group_id: Uuid,
	kind: &str,
	rank: &str,
	archived: bool,
) -> Uuid {
	let host = format!("http://repair.invalid/{}", Uuid::new_v4());
	let row: RowId = sql_query(
		"INSERT INTO servers (host, kind, rank, group_id, product, deleted_at) \
		 VALUES ($1, $2, $3, $4, 'tamanu', CASE WHEN $5 THEN now() ELSE NULL END) \
		 RETURNING id",
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Text, _>(kind)
	.bind::<sql_types::Text, _>(rank)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Bool, _>(archived)
	.get_result(conn)
	.await
	.expect("insert member");
	row.id
}

async fn report_version(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	version: &str,
) {
	sql_query(
		"INSERT INTO server_reported_detail (server_id, source, version, reported_at) \
		 VALUES ($1, 'alertd', $2, now())",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(version)
	.execute(conn)
	.await
	.expect("report version");
}

async fn set_cache(
	conn: &mut diesel_async::AsyncPgConnection,
	group_id: Uuid,
	server_id: Option<Uuid>,
	version: Option<&str>,
) {
	sql_query(
		"UPDATE server_groups SET version_server_id = $2, effective_version = $3 WHERE id = $1",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(server_id)
	.bind::<sql_types::Nullable<sql_types::Text>, _>(version)
	.execute(conn)
	.await
	.expect("set cache");
}

async fn cache(
	conn: &mut diesel_async::AsyncPgConnection,
	group_id: Uuid,
) -> (Option<Uuid>, Option<String>) {
	use database::schema::server_groups::dsl;
	use diesel::{ExpressionMethods, QueryDsl};
	dsl::server_groups
		.select((dsl::version_server_id, dsl::effective_version))
		.filter(dsl::id.eq(group_id))
		.first(conn)
		.await
		.expect("load cache")
}

/// The audit's example: a `live` central beside a `test` box. The original
/// backfill sent `live` to the `ELSE 5` bucket, so the test server outranked
/// the production one and spoke for the group.
#[tokio::test(flavor = "multi_thread")]
async fn the_repair_prefers_an_aliased_production_rank() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "aliased").await;
		let live = insert_member(&mut conn, group, "central", "live", false).await;
		let test = insert_member(&mut conn, group, "central", "test", false).await;
		report_version(&mut conn, live, "2.40.0").await;
		report_version(&mut conn, test, "1.0.0-test").await;

		// What the original backfill left behind.
		set_cache(&mut conn, group, Some(test), Some("1.0.0-test")).await;

		conn.batch_execute(REPAIR_UP).await.expect("replay repair");

		let (vsid, ver) = cache(&mut conn, group).await;
		assert_eq!(vsid, Some(live), "`live` is a production rank");
		assert_eq!(ver.as_deref(), Some("2.40.0"));
	})
	.await
}

/// `staging` and `prod` are the other two spellings `ServerRank::from_str`
/// accepts and the original `CASE` didn't.
#[tokio::test(flavor = "multi_thread")]
async fn the_repair_handles_the_other_rank_aliases() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "more-aliases").await;
		let prod = insert_member(&mut conn, group, "central", "prod", false).await;
		let staging = insert_member(&mut conn, group, "central", "staging", false).await;
		report_version(&mut conn, prod, "3.0.0").await;
		report_version(&mut conn, staging, "3.1.0-rc").await;

		set_cache(&mut conn, group, Some(staging), Some("3.1.0-rc")).await;
		conn.batch_execute(REPAIR_UP).await.expect("replay repair");

		let (vsid, _) = cache(&mut conn, group).await;
		assert_eq!(vsid, Some(prod), "`prod` outranks `staging`");
	})
	.await
}

/// The other half of the finding: the original selected among all servers,
/// so an archived one could be chosen as canonical.
#[tokio::test(flavor = "multi_thread")]
async fn the_repair_ignores_archived_members() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "archived").await;
		let gone = insert_member(&mut conn, group, "central", "production", true).await;
		let alive = insert_member(&mut conn, group, "facility", "production", false).await;
		report_version(&mut conn, gone, "1.0.0").await;
		report_version(&mut conn, alive, "2.0.0").await;

		set_cache(&mut conn, group, Some(gone), Some("1.0.0")).await;
		conn.batch_execute(REPAIR_UP).await.expect("replay repair");

		let (vsid, ver) = cache(&mut conn, group).await;
		assert_eq!(
			vsid,
			Some(alive),
			"an archived server can't speak for a group"
		);
		assert_eq!(ver.as_deref(), Some("2.0.0"));
	})
	.await
}

/// A group left with no eligible member keeps a stale pointer forever: the
/// original backfill's UPDATE only touched groups its CTE matched, so it
/// could never clear one.
#[tokio::test(flavor = "multi_thread")]
async fn the_repair_clears_a_group_with_no_eligible_member() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "emptied").await;
		let gone = insert_member(&mut conn, group, "central", "production", true).await;
		report_version(&mut conn, gone, "1.0.0").await;

		set_cache(&mut conn, group, Some(gone), Some("1.0.0")).await;
		conn.batch_execute(REPAIR_UP).await.expect("replay repair");

		let (vsid, ver) = cache(&mut conn, group).await;
		assert_eq!(vsid, None, "no live member, so no canonical member");
		assert_eq!(ver, None);
	})
	.await
}

/// And the repair agrees with the live code it mirrors: recomputing after it
/// changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_repair_agrees_with_recompute_version() {
	TestDb::run(async |mut conn, _| {
		let group = insert_group(&mut conn, "agreement").await;
		let live = insert_member(&mut conn, group, "central", "live", false).await;
		insert_member(&mut conn, group, "facility", "dev", false).await;
		insert_member(&mut conn, group, "central", "production", true).await;
		report_version(&mut conn, live, "4.5.6").await;

		conn.batch_execute(REPAIR_UP).await.expect("replay repair");
		let after_repair = cache(&mut conn, group).await;

		database::server_groups::ServerGroup::recompute_version(&mut conn, group)
			.await
			.expect("recompute");
		let after_recompute = cache(&mut conn, group).await;

		assert_eq!(
			after_repair, after_recompute,
			"the migration must not disagree with the code it mirrors",
		);
		assert_eq!(after_repair.0, Some(live));
	})
	.await
}
