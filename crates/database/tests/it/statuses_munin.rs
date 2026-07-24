use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use database::statuses::Status;

#[derive(QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

/// `extra` and `created_at` are test-controlled SQL literals, interpolated
/// directly; the server id is bound.
async fn insert_status(
	conn: &mut AsyncPgConnection,
	server_id: Uuid,
	extra: &str,
	created_at: &str,
) {
	let query = format!(
		"INSERT INTO statuses (server_id, extra, created_at) VALUES ($1, {extra}::jsonb, {created_at})"
	);
	sql_query(query)
		.bind::<sql_types::Uuid, _>(server_id)
		.execute(conn)
		.await
		.expect("insert status");
}

// spec: SVC#munin-link
#[tokio::test(flavor = "multi_thread")]
async fn munin_flag_read_with_grace() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let host = format!("http://munin.invalid/{}", Uuid::new_v4());
		let server: RowId = sql_query("INSERT INTO servers (host) VALUES ($1) RETURNING id")
			.bind::<sql_types::Text, _>(host)
			.get_result(&mut conn)
			.await
			.expect("insert server");
		let server_id = server.id;

		// Never reported → None (no link).
		assert_eq!(
			Status::latest_munin_for_server(&mut conn, server_id)
				.await
				.expect("query munin"),
			None,
		);

		// munin=true reported 20 days ago — older than the live 7-day window —
		// then a more recent status that omits the flag entirely.
		insert_status(
			&mut conn,
			server_id,
			r#"'{"munin": true}'"#,
			"NOW() - INTERVAL '20 days'",
		)
		.await;
		insert_status(&mut conn, server_id, "'{}'", "NOW() - INTERVAL '2 days'").await;

		assert_eq!(
			Status::latest_munin_for_server(&mut conn, server_id)
				.await
				.expect("query munin"),
			Some(true),
			"an old munin=true survives the 7-day window, and a later flag-less status doesn't disturb it",
		);

		// A later explicit munin=false overrides the earlier true.
		insert_status(&mut conn, server_id, r#"'{"munin": false}'"#, "NOW()").await;

		assert_eq!(
			Status::latest_munin_for_server(&mut conn, server_id)
				.await
				.expect("query munin"),
			Some(false),
			"a later explicit munin=false overrides the earlier true",
		);
	})
	.await
}
