//! HTTP tests for the managed-restore endpoints (backup-restore role). The
//! worklist/capability paths run against the standard harness (no STS/kube
//! needed); restore-credentials is covered for its authz (403) and the
//! authorized-but-unconfigured (502) paths.

use diesel::{sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

async fn make_group(conn: &mut AsyncPgConnection) -> Uuid {
	let id = Uuid::new_v4();
	sql_query("INSERT INTO server_groups (id, name) VALUES ($1, 'restore-test-group')")
		.bind::<sql_types::Uuid, _>(id)
		.execute(conn)
		.await
		.expect("insert group");
	id
}

async fn make_config(conn: &mut AsyncPgConnection, group_id: Uuid, status: &str) {
	sql_query(
		"INSERT INTO server_group_backup_config \
		 (group_id, bucket, prefix, target_role_arn, maintenance_role_arn, region, repo_password_ref, status) \
		 VALUES ($1, 'grp-bucket', '', 'arn:aws:iam::123456789012:role/grp', 'arn:aws:iam::123456789012:role/grp-maint', 'ap-southeast-2', 'grp-repo-pw', $2)",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Text, _>(status)
	.execute(conn)
	.await
	.expect("insert config");
}

async fn make_server(conn: &mut AsyncPgConnection, group_id: Uuid) -> Uuid {
	let server_id = Uuid::new_v4();
	let host = format!("https://srv-{server_id}.example.com");
	sql_query("INSERT INTO servers (id, host, kind, group_id) VALUES ($1, $2, 'central', $3)")
		.bind::<sql_types::Uuid, _>(server_id)
		.bind::<sql_types::Text, _>(host)
		.bind::<sql_types::Uuid, _>(group_id)
		.execute(conn)
		.await
		.expect("insert server");
	server_id
}

/// A successful `backup` run = the snapshot the worklist should surface.
async fn make_success_run(
	conn: &mut AsyncPgConnection,
	device_id: Uuid,
	group_id: Uuid,
	server_id: Uuid,
	snapshot_id: &str,
) {
	sql_query(
		"INSERT INTO backup_runs (id, device_id, group_id, server_id, type, purpose, outcome, snapshot_id) \
		 VALUES ($1, $2, $3, $4, 'tamanu-postgres', 'backup', 'success', $5)",
	)
	.bind::<sql_types::Uuid, _>(Uuid::new_v4())
	.bind::<sql_types::Uuid, _>(device_id)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(snapshot_id)
	.execute(conn)
	.await
	.expect("insert run");
}

async fn declare_replica(
	conn: &mut AsyncPgConnection,
	consumer: Uuid,
	group_id: Uuid,
	intent: &str,
) {
	sql_query(
		"INSERT INTO restore_replicas (consumer_device_id, group_id, type, intent, name) \
		 VALUES ($1, $2, 'tamanu-postgres', $3, $4)",
	)
	.bind::<sql_types::Uuid, _>(consumer)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Text, _>(intent)
	.bind::<sql_types::Text, _>(format!("{intent}-decl"))
	.execute(conn)
	.await
	.expect("insert declaration");
}

async fn declare_replica_server(
	conn: &mut AsyncPgConnection,
	consumer: Uuid,
	group_id: Uuid,
	server_id: Uuid,
	intent: &str,
) {
	sql_query(
		"INSERT INTO restore_replicas (consumer_device_id, group_id, server_id, type, intent, name) \
		 VALUES ($1, $2, $3, 'tamanu-postgres', $4, $5)",
	)
	.bind::<sql_types::Uuid, _>(consumer)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(intent)
	.bind::<sql_types::Text, _>(format!("{intent}-server-decl"))
	.execute(conn)
	.await
	.expect("insert server declaration");
}

#[tokio::test(flavor = "multi_thread")]
async fn capabilities_register_then_worklist_filters_by_intent() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_config(&mut conn, group, "ready").await;
			let server = make_server(&mut conn, group).await;
			make_success_run(&mut conn, device_id, group, server, "snap-1").await;

			// Two whole-group declarations, different intents.
			declare_replica(&mut conn, device_id, group, "verify").await;
			declare_replica(&mut conn, device_id, group, "analytics").await;

			// Register only `verify`.
			public
				.post("/restore-capabilities")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "intents": ["verify"] }))
				.await
				.assert_status(http::StatusCode::NO_CONTENT);

			let resp = public
				.get("/restore-worklist")
				.add_header("mtls-certificate", &cert)
				.await;
			resp.assert_status_ok();
			let entries: Vec<serde_json::Value> = resp.json();
			// Only the `verify` declaration is dispatched; `analytics` is a gap.
			assert_eq!(entries.len(), 1, "got {entries:?}");
			assert_eq!(entries[0]["intent"], "verify");
			assert_eq!(entries[0]["server_id"], server.to_string());
			assert_eq!(entries[0]["snapshot_id"], "snap-1");
			assert_eq!(entries[0]["bucket"], "grp-bucket");
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn worklist_expands_group_wide_to_each_server() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_config(&mut conn, group, "ready").await;
			let server_a = make_server(&mut conn, group).await;
			let server_b = make_server(&mut conn, group).await;
			make_success_run(&mut conn, device_id, group, server_a, "snap-a").await;
			make_success_run(&mut conn, device_id, group, server_b, "snap-b").await;
			declare_replica(&mut conn, device_id, group, "verify").await;
			public
				.post("/restore-capabilities")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "intents": ["verify"] }))
				.await
				.assert_status(http::StatusCode::NO_CONTENT);

			let resp = public
				.get("/restore-worklist")
				.add_header("mtls-certificate", &cert)
				.await;
			resp.assert_status_ok();
			let entries: Vec<serde_json::Value> = resp.json();
			// One whole-group declaration → one entry per live server, each with
			// its own latest snapshot.
			assert_eq!(entries.len(), 2, "got {entries:?}");
			let mut by_server: std::collections::HashMap<String, String> = entries
				.iter()
				.map(|e| {
					(
						e["server_id"].as_str().unwrap().to_owned(),
						e["snapshot_id"].as_str().unwrap().to_owned(),
					)
				})
				.collect();
			assert_eq!(
				by_server.remove(&server_a.to_string()).as_deref(),
				Some("snap-a")
			);
			assert_eq!(
				by_server.remove(&server_b.to_string()).as_deref(),
				Some("snap-b")
			);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn worklist_dedupes_server_specific_over_group_wide() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_config(&mut conn, group, "ready").await;
			let server = make_server(&mut conn, group).await;
			make_success_run(&mut conn, device_id, group, server, "snap-1").await;
			// Both a whole-group and a server-specific declaration of the same
			// (type, intent) cover this server.
			declare_replica(&mut conn, device_id, group, "verify").await;
			declare_replica_server(&mut conn, device_id, group, server, "verify").await;
			public
				.post("/restore-capabilities")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "intents": ["verify"] }))
				.await
				.assert_status(http::StatusCode::NO_CONTENT);

			let resp = public
				.get("/restore-worklist")
				.add_header("mtls-certificate", &cert)
				.await;
			resp.assert_status_ok();
			let entries: Vec<serde_json::Value> = resp.json();
			// Deduped to a single entry for the server, not two.
			assert_eq!(entries.len(), 1, "got {entries:?}");
			assert_eq!(entries[0]["server_id"], server.to_string());
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn worklist_empty_without_registered_capabilities() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_config(&mut conn, group, "ready").await;
			make_server(&mut conn, group).await;
			declare_replica(&mut conn, device_id, group, "verify").await;

			// No capabilities registered → nothing dispatched.
			let resp = public
				.get("/restore-worklist")
				.add_header("mtls-certificate", &cert)
				.await;
			resp.assert_status_ok();
			let entries: Vec<serde_json::Value> = resp.json();
			assert!(entries.is_empty(), "got {entries:?}");
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_credentials_without_declaration_is_403() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, _device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_config(&mut conn, group, "ready").await;
			let resp = public
				.post("/restore-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "group": group, "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::FORBIDDEN);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_credentials_authorized_but_unconfigured_is_502() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_config(&mut conn, group, "ready").await;
			declare_replica(&mut conn, device_id, group, "verify").await;

			// Authorization passes; the harness has no STS client, so issuance
			// fails upstream rather than 403.
			let resp = public
				.post("/restore-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "group": group, "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::BAD_GATEWAY);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_endpoints_reject_non_consumer_role() {
	// A `server`-role device cannot reach the backup-restore endpoints.
	commons_tests::server::run_with_device_auth(
		"server",
		async |_conn, cert, _device_id, public, _| {
			let resp = public
				.get("/restore-worklist")
				.add_header("mtls-certificate", &cert)
				.await;
			resp.assert_status(http::StatusCode::FORBIDDEN);
		},
	)
	.await;
}

#[derive(diesel::QueryableByName)]
struct Count {
	#[diesel(sql_type = sql_types::BigInt)]
	count: i64,
}

async fn count(conn: &mut AsyncPgConnection, query: &str, group: Uuid) -> i64 {
	sql_query(query)
		.bind::<sql_types::Uuid, _>(group)
		.get_result::<Count>(conn)
		.await
		.expect("count")
		.count
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_verification_without_declaration_is_403() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, _device_id, public, _| {
			let group = make_group(&mut conn).await;
			let server = make_server(&mut conn, group).await;
			let resp = public
				.post("/restore-verification")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"group": group,
					"server_id": server,
					"type": "tamanu-postgres",
					"intent": "verify",
					"outcome": "failure",
					"replica_healthy": false,
					"observed_at": "2026-06-30T00:00:00Z",
				}))
				.await;
			resp.assert_status(http::StatusCode::FORBIDDEN);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_verification_records_and_raises_alert() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			let server = make_server(&mut conn, group).await;
			declare_replica(&mut conn, device_id, group, "verify").await;

			public
				.post("/restore-verification")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"group": group,
					"server_id": server,
					"type": "tamanu-postgres",
					"intent": "verify",
					"snapshot_id": "snap-1",
					"outcome": "failure",
					"error": "restore failed",
					"replica_healthy": false,
					"observed_at": "2026-06-30T00:00:00Z",
				}))
				.await
				.assert_status(http::StatusCode::NO_CONTENT);

			assert_eq!(
				count(
					&mut conn,
					"SELECT count(*) AS count FROM backup_restore_checks WHERE group_id = $1",
					group,
				)
				.await,
				1,
			);
			assert_eq!(
				count(
					&mut conn,
					"SELECT count(*) AS count FROM issues WHERE server_group_id = $1 \
					 AND ref LIKE 'restore-verification:%' AND active = true",
					group,
				)
				.await,
				1,
			);
		},
	)
	.await;
}
