use commons_errors::Result;
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};

/// Distinct top-level tag keys across all servers and server groups,
/// sorted. Used by the admin rule editor to populate autocomplete
/// suggestions for `tag.*` variables — a key that exists on any
/// server or group is a valid thing to predicate on, even if the
/// sampled server doesn't carry it.
pub async fn all_known_keys(conn: &mut AsyncPgConnection) -> Result<Vec<String>> {
	#[derive(QueryableByName)]
	struct Row {
		#[diesel(sql_type = sql_types::Text)]
		key: String,
	}
	let rows: Vec<Row> = sql_query(
		"SELECT key FROM ( \
		    SELECT jsonb_object_keys(tags) AS key FROM servers \
		    UNION SELECT jsonb_object_keys(tags) AS key FROM server_groups \
		 ) k ORDER BY key",
	)
	.get_results(conn)
	.await
	.map_err(commons_errors::AppError::from)?;
	Ok(rows.into_iter().map(|r| r.key).collect())
}
