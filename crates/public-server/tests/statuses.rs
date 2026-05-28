use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
struct StatusResult {
	#[diesel(sql_type = sql_types::Uuid)]
	server_id: Uuid,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
	device_id: Option<Uuid>,
	#[diesel(sql_type = sql_types::Jsonb)]
	extra: serde_json::Value,
}

#[derive(QueryableByName)]
struct HealthResult {
	#[diesel(sql_type = sql_types::Bool)]
	healthy: bool,
	#[diesel(sql_type = sql_types::Jsonb)]
	health: serde_json::Value,
	#[diesel(sql_type = sql_types::Jsonb)]
	extra: serde_json::Value,
}

#[derive(QueryableByName, Debug)]
struct IssueRow {
	#[diesel(sql_type = sql_types::Text)]
	severity: String,
	#[diesel(sql_type = sql_types::Bool)]
	active: bool,
	#[diesel(sql_type = sql_types::Text)]
	message: String,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	description: Option<String>,
	#[diesel(sql_type = sql_types::Bool)]
	is_resolved: bool,
}

async fn fetch_issue(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
	source: &str,
	r#ref: &str,
) -> Option<IssueRow> {
	sql_query(
		r#"
		SELECT severity, active, message, description, (resolved_at IS NOT NULL) AS is_resolved
		FROM issues
		WHERE server_id = $1 AND source = $2 AND ref = $3
"#,
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(source)
	.bind::<sql_types::Text, _>(r#ref)
	.get_result(conn)
	.await
	.ok()
}

#[derive(QueryableByName, Debug)]
struct EventCount {
	#[diesel(sql_type = sql_types::BigInt)]
	count: i64,
}

async fn count_issues_for_server(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
) -> i64 {
	let c: EventCount = sql_query("SELECT COUNT(*) AS count FROM issues WHERE server_id = $1")
		.bind::<sql_types::Uuid, _>(server_id)
		.get_result(conn)
		.await
		.expect("count issues");
	c.count
}

#[derive(QueryableByName, Debug)]
struct IncidentRow {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

/// Pre-seed (or update) a catalog row so a check's failure severity is
/// known up-front. v1 ingestion would otherwise auto-insert at the
/// default Warning, which only opens an incident when one already
/// exists. Tests that want to exercise Error-class behaviour seed
/// here.
async fn set_check_severity(
	conn: &mut diesel_async::AsyncPgConnection,
	check_name: &str,
	severity: &str,
) {
	sql_query(
		"INSERT INTO healthcheck_severities (check_name, severity, reviewed_at, reviewed_by) \
		 VALUES ($1, $2, NOW(), 'test') \
		 ON CONFLICT (check_name) DO UPDATE \
		 SET severity = EXCLUDED.severity, \
		     reviewed_at = EXCLUDED.reviewed_at, \
		     reviewed_by = EXCLUDED.reviewed_by",
	)
	.bind::<sql_types::Text, _>(check_name)
	.bind::<sql_types::Text, _>(severity)
	.execute(conn)
	.await
	.expect("seed catalog severity");
}

async fn fetch_open_incident(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
) -> Option<IncidentRow> {
	sql_query(
		"SELECT i.id FROM incidents i \
		 JOIN servers s ON i.server_group_id = s.group_id \
		 WHERE s.id = $1 AND i.closed_at IS NULL",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.get_result(conn)
	.await
	.ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				r#"
				INSERT INTO servers (id, host, kind, device_id)
				VALUES ($1, 'https://test.example.com', 'facility', $2)
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
			.execute(&mut conn)
			.await
			.expect("insert server");

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "uptime": 3600 }))
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
			let returned_status: serde_json::Value = response.json();
			assert!(returned_status.get("id").is_some());
			assert_eq!(
				returned_status.get("server_id").and_then(|v| v.as_str()),
				Some(server_id.to_string().as_str())
			);
			assert_eq!(
				returned_status.get("device_id").and_then(|v| v.as_str()),
				Some(device_id.to_string().as_str())
			);
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(3600));

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
				FROM statuses
				WHERE server_id = $1
				ORDER BY created_at DESC
				LIMIT 1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch created status");

			assert_eq!(db_status.server_id, server_id);
			assert_eq!(db_status.device_id, Some(device_id));
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(3600)
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_geolocation() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				r#"
				INSERT INTO servers (id, host, kind, device_id, geolocation)
				VALUES ($1, 'https://test.example.com', 'facility', $2, ARRAY[-41.2865, 174.7762])
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
			.execute(&mut conn)
			.await
			.expect("insert server with geolocation");

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "uptime": 7200, "version": "2.8.1" }))
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
			let returned_status: serde_json::Value = response.json();
			assert!(returned_status.get("id").is_some());
			assert_eq!(
				returned_status.get("server_id").and_then(|v| v.as_str()),
				Some(server_id.to_string().as_str())
			);
			assert_eq!(
				returned_status.get("device_id").and_then(|v| v.as_str()),
				Some(device_id.to_string().as_str())
			);
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(7200));
			assert_eq!(extra.get("version").and_then(|v| v.as_str()), Some("2.8.1"));

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
				FROM statuses
				WHERE server_id = $1
				ORDER BY created_at DESC
				LIMIT 1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch created status");

			assert_eq!(db_status.server_id, server_id);
			assert_eq!(db_status.device_id, Some(device_id));
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(7200)
			);
			assert_eq!(
				db_status.extra.get("version").and_then(|v| v.as_str()),
				Some("2.8.1")
			);

			// Verify server still has geolocation
			#[derive(QueryableByName)]
			struct GeoCheck {
				#[diesel(sql_type = sql_types::Bool)]
				has_geolocation: bool,
			}

			let server_with_geo: GeoCheck = sql_query(
				r#"
				SELECT geolocation IS NOT NULL as has_geolocation
				FROM servers
				WHERE id = $1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch server geolocation status");

			assert!(
				server_with_geo.has_geolocation,
				"Server should have geolocation"
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_cloud() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				r#"
				INSERT INTO servers (id, host, kind, device_id, cloud)
				VALUES ($1, 'https://cloud.example.com', 'central', $2, true)
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
			.execute(&mut conn)
			.await
			.expect("insert server with cloud");

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "uptime": 4800, "platform": "Linux" }))
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
			let returned_status: serde_json::Value = response.json();
			assert!(returned_status.get("id").is_some());
			assert_eq!(
				returned_status.get("server_id").and_then(|v| v.as_str()),
				Some(server_id.to_string().as_str())
			);
			assert_eq!(
				returned_status.get("device_id").and_then(|v| v.as_str()),
				Some(device_id.to_string().as_str())
			);
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(4800));
			assert_eq!(
				extra.get("platform").and_then(|v| v.as_str()),
				Some("Linux")
			);

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
				FROM statuses
				WHERE server_id = $1
				ORDER BY created_at DESC
				LIMIT 1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch created status");

			assert_eq!(db_status.server_id, server_id);
			assert_eq!(db_status.device_id, Some(device_id));
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(4800)
			);
			assert_eq!(
				db_status.extra.get("platform").and_then(|v| v.as_str()),
				Some("Linux")
			);

			// Verify server still has cloud field set to true
			#[derive(QueryableByName)]
			struct CloudCheck {
				#[diesel(sql_type = sql_types::Nullable<sql_types::Bool>)]
				cloud: Option<bool>,
			}

			let server_with_cloud: CloudCheck = sql_query(
				r#"
				SELECT cloud
				FROM servers
				WHERE id = $1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch server cloud status");

			assert_eq!(
				server_with_cloud.cloud,
				Some(true),
				"Server should have cloud=true"
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_geolocation_and_cloud() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				r#"
				INSERT INTO servers (id, host, kind, device_id, geolocation, cloud)
				VALUES ($1, 'https://full.example.com', 'central', $2, ARRAY[40.7128, -74.0060], false)
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
			.execute(&mut conn)
			.await
			.expect("insert server with geolocation and cloud");

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(
					&serde_json::json!({ "uptime": 10000, "version": "3.0.0", "timezone": "America/New_York" }),
				)
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
			let returned_status: serde_json::Value = response.json();
			assert!(returned_status.get("id").is_some());
			assert_eq!(
				returned_status.get("server_id").and_then(|v| v.as_str()),
				Some(server_id.to_string().as_str())
			);
			assert_eq!(
				returned_status.get("device_id").and_then(|v| v.as_str()),
				Some(device_id.to_string().as_str())
			);
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(10000));
			assert_eq!(extra.get("version").and_then(|v| v.as_str()), Some("3.0.0"));
			assert_eq!(
				extra.get("timezone").and_then(|v| v.as_str()),
				Some("America/New_York")
			);

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
				FROM statuses
				WHERE server_id = $1
				ORDER BY created_at DESC
				LIMIT 1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch created status");

			assert_eq!(db_status.server_id, server_id);
			assert_eq!(db_status.device_id, Some(device_id));
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(10000)
			);
			assert_eq!(
				db_status.extra.get("version").and_then(|v| v.as_str()),
				Some("3.0.0")
			);
			assert_eq!(
				db_status.extra.get("timezone").and_then(|v| v.as_str()),
				Some("America/New_York")
			);

			// Verify server still has both geolocation and cloud fields
			#[derive(QueryableByName)]
			struct FullCheck {
				#[diesel(sql_type = sql_types::Nullable<sql_types::Array<sql_types::Float8>>)]
				geolocation: Option<Vec<f64>>,
				#[diesel(sql_type = sql_types::Nullable<sql_types::Bool>)]
				cloud: Option<bool>,
			}

			let server_check: FullCheck = sql_query(
				r#"
				SELECT geolocation, cloud
				FROM servers
				WHERE id = $1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch server geolocation and cloud status");

			assert!(
				server_check.geolocation.is_some(),
				"Server should have geolocation"
			);
			if let Some(geo) = &server_check.geolocation {
				assert_eq!(geo.len(), 2, "Geolocation should have 2 values");
				assert!(
					(geo[0] - 40.7128).abs() < 0.0001,
					"Latitude should be ~40.7128"
				);
				assert!(
					(geo[1] - (-74.0060)).abs() < 0.0001,
					"Longitude should be ~-74.0060"
				);
			}

			assert_eq!(
				server_check.cloud,
				Some(false),
				"Server should have cloud=false"
			);
		},
	)
	.await
}

/// Helper used by the health-payload tests below. Bare server with a device
/// attached, no extras. Returns the created server's id.
async fn insert_health_test_server(
	conn: &mut diesel_async::AsyncPgConnection,
	device_id: Uuid,
) -> Uuid {
	// Server gets a group so events promote to incidents normally.
	let group_id = Uuid::new_v4();
	sql_query("INSERT INTO server_groups (id, name) VALUES ($1, 'health-group')")
		.bind::<sql_types::Uuid, _>(group_id)
		.execute(conn)
		.await
		.expect("insert group");
	let server_id = Uuid::new_v4();
	sql_query(
		r#"
		INSERT INTO servers (id, host, kind, device_id, group_id)
		VALUES ($1, 'https://health.example.com', 'central', $2, $3)
	"#,
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
	.bind::<sql_types::Uuid, _>(group_id)
	.execute(conn)
	.await
	.expect("insert server");
	server_id
}

async fn fetch_latest_health(
	conn: &mut diesel_async::AsyncPgConnection,
	server_id: Uuid,
) -> HealthResult {
	sql_query(
		r#"
		SELECT healthy, health, extra
		FROM statuses
		WHERE server_id = $1
		ORDER BY created_at DESC
		LIMIT 1
	"#,
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.get_result(conn)
	.await
	.expect("fetch latest health")
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_legacy_no_healthy_field() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "uptime": 100 }))
				.await;
			response.assert_status_ok();

			let row = fetch_latest_health(&mut conn, server_id).await;
			assert!(row.healthy, "absent `healthy` ⇒ true (legacy compat)");
			assert_eq!(row.health, serde_json::json!([]));
			assert_eq!(row.extra.get("uptime").and_then(|v| v.as_i64()), Some(100));
			assert!(
				row.extra.get("healthy").is_none(),
				"`healthy` must not leak into `extra`"
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_healthy_true_no_checks() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "healthy": true }))
				.await;
			response.assert_status_ok();

			let row = fetch_latest_health(&mut conn, server_id).await;
			assert!(row.healthy);
			assert_eq!(row.health, serde_json::json!([]));
			assert_eq!(row.extra, serde_json::json!({}));
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_healthy_false_persists() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"healthy": false,
					"uptime": 42,
				}))
				.await;
			response.assert_status_ok();

			let row = fetch_latest_health(&mut conn, server_id).await;
			assert!(!row.healthy);
			assert_eq!(row.extra.get("uptime").and_then(|v| v.as_i64()), Some(42));
			assert!(row.extra.get("healthy").is_none());
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_health_checks_persists() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"healthy": true,
					"health": [
						{ "check": "database", "healthy": true, "latency_ms": 12 },
						{ "check": "disk", "healthy": false, "free_pct": 4, "message": "almost full" },
					],
					"timezone": "UTC",
				}))
				.await;
			response.assert_status_ok();

			let row = fetch_latest_health(&mut conn, server_id).await;
			assert!(row.healthy);
			let arr = row.health.as_array().expect("array");
			assert_eq!(arr.len(), 2);
			assert_eq!(arr[0]["check"], "database");
			assert_eq!(arr[0]["latency_ms"], 12);
			assert_eq!(arr[1]["check"], "disk");
			assert_eq!(arr[1]["free_pct"], 4);
			assert_eq!(arr[1]["message"], "almost full");
			assert!(
				row.extra.get("health").is_none(),
				"`health` must not leak into `extra`"
			);
			assert_eq!(
				row.extra.get("timezone").and_then(|v| v.as_str()),
				Some("UTC")
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_rejects_non_bool_healthy() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "healthy": "yes" }))
				.await;
			response.assert_status_bad_request();
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_rejects_non_array_health() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "health": { "check": "x", "healthy": true } }))
				.await;
			response.assert_status_bad_request();
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_rejects_health_entry_missing_check() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"health": [ { "healthy": true } ]
				}))
				.await;
			response.assert_status_bad_request();
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_rejects_health_entry_missing_healthy() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"health": [ { "check": "database" } ]
				}))
				.await;
			response.assert_status_bad_request();
		},
	)
	.await
}

// -----------------------------------------------------------------
// Phase 3 — event filing on healthy transitions.
// -----------------------------------------------------------------

async fn post_status(
	public: &axum_test::TestServer,
	cert: &str,
	server_id: Uuid,
	body: serde_json::Value,
) {
	let response = public
		.post(&format!("/status/{}", server_id))
		.add_header("mtls-certificate", cert)
		.json(&body)
		.await;
	response.assert_status_ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_legacy_files_no_health_issues() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({ "uptime": 1 }),
			)
			.await;

			assert_eq!(
				count_issues_for_server(&mut conn, server_id).await,
				0,
				"legacy push (no healthy field) must not open any status/* issue"
			);
			assert!(fetch_open_incident(&mut conn, server_id).await.is_none());
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_warning_check_only() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": true,
					"health": [ { "check": "disk", "healthy": false, "free_pct": 4 } ],
				}),
			)
			.await;

			let per_check = fetch_issue(&mut conn, server_id, "status", "health/disk")
				.await
				.expect("per-check issue filed");
			assert_eq!(per_check.severity, "warning");
			assert!(per_check.active);
			assert!(!per_check.is_resolved);
			assert!(
				per_check
					.description
					.as_ref()
					.is_some_and(|d| d.contains("disk"))
			);
			assert!(per_check.message.contains("free_pct"));

			// Rollup is retired — never filed in any case.
			assert!(
				fetch_issue(&mut conn, server_id, "status", "health")
					.await
					.is_none()
			);
			// Warning alone doesn't cross the incident threshold.
			assert!(fetch_open_incident(&mut conn, server_id).await.is_none());
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_unhealthy_with_checks_opens_incident() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			// Operator has elevated these to Error in the catalog. Without
			// this seed, both would default to Warning and not open an
			// incident on their own.
			set_check_severity(&mut conn, "database", "error").await;
			set_check_severity(&mut conn, "disk", "error").await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [
						{ "check": "database", "healthy": false, "lag_ms": 5000 },
						{ "check": "disk", "healthy": false },
						{ "check": "tls", "healthy": true },
					],
				}),
			)
			.await;

			// The (status, health) rollup was retired — only per-check issues now.
			assert!(
				fetch_issue(&mut conn, server_id, "status", "health")
					.await
					.is_none(),
				"rollup issue must not be created"
			);

			for check in ["database", "disk"] {
				let i = fetch_issue(&mut conn, server_id, "status", &format!("health/{check}"))
					.await
					.unwrap_or_else(|| panic!("per-check issue for {check} missing"));
				assert_eq!(i.severity, "error", "{check}");
				assert!(i.active, "{check}");
			}
			// Passing check shouldn't manifest as a resolved-from-birth issue.
			assert!(
				fetch_issue(&mut conn, server_id, "status", "health/tls")
					.await
					.is_none(),
				"passing check must not create an issue"
			);
			// Per-check failures at the catalog's Error severity open an incident.
			assert!(fetch_open_incident(&mut conn, server_id).await.is_some());
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_unhealthy_no_checks_files_nothing() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({ "healthy": false }),
			)
			.await;

			// The retired rollup is no longer filed and there are no per-check
			// failures to file individual issues against, so the unhealthy flag
			// on its own produces nothing.
			assert!(
				fetch_issue(&mut conn, server_id, "status", "health")
					.await
					.is_none(),
				"rollup issue must not be created"
			);
			assert_eq!(count_issues_for_server(&mut conn, server_id).await, 0);
			assert!(fetch_open_incident(&mut conn, server_id).await.is_none());
		},
	)
	.await
}

/// All failing checks are silenced at server scope → per-check issues
/// are still recorded (silence doesn't suppress the row) but none of
/// them join an incident. The retired rollup no longer factors in.
#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_all_failing_checks_silenced_opens_no_incident() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			// Pre-silence both failing checks at server scope.
			for check in ["database", "disk"] {
				sql_query(
					"INSERT INTO server_silenced_refs (server_id, source, ref) \
					 VALUES ($1, 'status', $2)",
				)
				.bind::<sql_types::Uuid, _>(server_id)
				.bind::<sql_types::Text, _>(format!("health/{check}"))
				.execute(&mut conn)
				.await
				.expect("seed silence");
			}

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [
						{ "check": "database", "healthy": false },
						{ "check": "disk", "healthy": false },
					],
				}),
			)
			.await;

			// `healthy` is persisted as-is now that the silence-proxy
			// correction is gone — operators see what bestool said.
			let row = fetch_latest_health(&mut conn, server_id).await;
			assert!(!row.healthy);

			// Per-check issues exist (silence doesn't gate row creation)
			// but the silence prevents them from joining an incident.
			for check in ["database", "disk"] {
				let i = fetch_issue(&mut conn, server_id, "status", &format!("health/{check}"))
					.await
					.unwrap_or_else(|| panic!("per-check issue for {check} missing"));
				assert!(i.active, "{check}");
			}
			assert!(fetch_open_incident(&mut conn, server_id).await.is_none());
		},
	)
	.await
}

/// Partial silence: the unsilenced failing check (operator-elevated to
/// Error in the catalog) is enough to open an incident.
#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_partial_silence_opens_incident_for_unsilenced() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			// Silence only one of the two failing checks.
			sql_query(
				"INSERT INTO server_silenced_refs (server_id, source, ref) \
				 VALUES ($1, 'status', 'health/database')",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.execute(&mut conn)
			.await
			.expect("seed silence");

			// Operator has elevated the unsilenced check to Error.
			set_check_severity(&mut conn, "disk", "error").await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [
						{ "check": "database", "healthy": false },
						{ "check": "disk", "healthy": false },
					],
				}),
			)
			.await;

			let row = fetch_latest_health(&mut conn, server_id).await;
			assert!(!row.healthy);
			assert!(
				fetch_issue(&mut conn, server_id, "status", "health")
					.await
					.is_none(),
				"rollup must not be created"
			);
			// Disk is unsilenced and configured at Error severity → opens an incident.
			let disk = fetch_issue(&mut conn, server_id, "status", "health/disk")
				.await
				.expect("disk issue filed");
			assert_eq!(disk.severity, "error");
			assert!(fetch_open_incident(&mut conn, server_id).await.is_some());
		},
	)
	.await
}

/// In v1, per-check severity comes from the catalog and is therefore
/// independent of bestool's top-level `healthy` flag. Two consecutive
/// pushes — one with healthy=false and one with healthy=true — file
/// the same check at the same severity.
#[tokio::test(flavor = "multi_thread")]
async fn submit_status_per_check_severity_is_catalog_driven() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			// First push: bestool reports overall unhealthy, but the
			// catalog hasn't been touched yet — disk defaults to Warning,
			// not Error.
			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [ { "check": "disk", "healthy": false } ],
				}),
			)
			.await;
			let after_first = fetch_issue(&mut conn, server_id, "status", "health/disk")
				.await
				.expect("per-check issue");
			assert_eq!(after_first.severity, "warning");

			// Second push: bestool reports overall healthy. Same severity —
			// the catalog is the source of truth, not the top-level flag.
			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": true,
					"health": [ { "check": "disk", "healthy": false } ],
				}),
			)
			.await;
			let disk = fetch_issue(&mut conn, server_id, "status", "health/disk")
				.await
				.expect("per-check issue still present");
			assert_eq!(disk.severity, "warning");
			assert!(disk.active, "still failing, must stay active");

			assert!(
				fetch_issue(&mut conn, server_id, "status", "health")
					.await
					.is_none(),
				"rollup is retired and must not exist"
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_check_recovery_explicit() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [ { "check": "db", "healthy": false } ],
				}),
			)
			.await;
			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": true,
					"health": [ { "check": "db", "healthy": true } ],
				}),
			)
			.await;

			let issue = fetch_issue(&mut conn, server_id, "status", "health/db")
				.await
				.expect("per-check issue exists");
			assert!(!issue.active, "explicit healthy=true closes the issue");
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_check_recovery_via_drop() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [ { "check": "db", "healthy": false } ],
				}),
			)
			.await;
			// Second push omits `db` from health[] entirely — trust the
			// reporter, treat as recovered.
			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": true,
					"health": [],
				}),
			)
			.await;

			let issue = fetch_issue(&mut conn, server_id, "status", "health/db")
				.await
				.expect("per-check issue exists");
			assert!(!issue.active);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_full_recovery_closes_incident() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			// Both checks need to be at Error for the initial push to open
			// an incident on its own; catalog default of Warning wouldn't.
			set_check_severity(&mut conn, "db", "error").await;
			set_check_severity(&mut conn, "disk", "error").await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [
						{ "check": "db", "healthy": false },
						{ "check": "disk", "healthy": false },
					],
				}),
			)
			.await;
			assert!(fetch_open_incident(&mut conn, server_id).await.is_some());

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": true,
					"health": [
						{ "check": "db", "healthy": true },
						{ "check": "disk", "healthy": true },
					],
				}),
			)
			.await;

			assert!(
				fetch_open_incident(&mut conn, server_id).await.is_none(),
				"incident must auto-close when every contributing issue resolves"
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_reachability_to_health_handoff() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			// db is at Error in the catalog so the per-check is a
			// meaningful contributor that can hold the incident open
			// once reachability resolves.
			set_check_severity(&mut conn, "db", "error").await;

			// Initial state: server silent → reachability sweep files a
			// canopy/reachability issue at Critical (severity for `Gone`),
			// which opens an incident on the server's group.
			database::statuses::Status::sweep_reachability(&mut conn)
				.await
				.expect("reachability sweep");
			let reach_before = fetch_issue(&mut conn, server_id, "canopy", "reachability")
				.await
				.expect("reachability issue opened");
			assert!(reach_before.active);
			let incident_before = fetch_open_incident(&mut conn, server_id)
				.await
				.expect("incident opened by reachability");

			// Server pings in with a failing per-check; the per-check
			// issue joins the existing incident rather than opening a
			// separate one.
			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [ { "check": "db", "healthy": false } ],
				}),
			)
			.await;

			let per_check = fetch_issue(&mut conn, server_id, "status", "health/db")
				.await
				.expect("per-check issue opened");
			assert!(per_check.active);
			let incident_after_push = fetch_open_incident(&mut conn, server_id)
				.await
				.expect("incident still open after health push");
			assert_eq!(
				incident_before.id, incident_after_push.id,
				"same incident absorbs both contributors"
			);

			// Reachability sweep runs again. Server's latest status is fresh
			// so the sweep closes the reachability issue. The incident must
			// stay open because the per-check issue is still contributing.
			database::statuses::Status::sweep_reachability(&mut conn)
				.await
				.expect("reachability sweep (recovery)");

			let reach_after = fetch_issue(&mut conn, server_id, "canopy", "reachability")
				.await
				.expect("reachability issue still queryable");
			assert!(!reach_after.active, "reachability auto-recovered");
			let incident_final = fetch_open_incident(&mut conn, server_id)
				.await
				.expect("incident stays open across reachability close");
			assert_eq!(incident_before.id, incident_final.id);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_keeps_incident_open_when_failure_swaps() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			// Operator has elevated db and disk to Error in the catalog so
			// either failing check opens an incident on its own.
			set_check_severity(&mut conn, "db", "error").await;
			set_check_severity(&mut conn, "disk", "error").await;

			// Prior state: db failing.
			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [ { "check": "db", "healthy": false } ],
				}),
			)
			.await;
			let incident_before = fetch_open_incident(&mut conn, server_id)
				.await
				.expect("incident opened by initial push");

			// New push: db has recovered, disk newly failing. Mix of opens
			// and closes in one push — the incident must stay open because
			// disk is now contributing in db's place.
			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [
						{ "check": "db", "healthy": true },
						{ "check": "disk", "healthy": false },
					],
				}),
			)
			.await;

			let incident_after = fetch_open_incident(&mut conn, server_id)
				.await
				.expect("incident must remain open");
			assert_eq!(
				incident_before.id, incident_after.id,
				"same incident, not a close+reopen flicker"
			);

			let db = fetch_issue(&mut conn, server_id, "status", "health/db")
				.await
				.expect("db issue");
			assert!(!db.active, "db has recovered");
			let disk = fetch_issue(&mut conn, server_id, "status", "health/disk")
				.await
				.expect("disk issue");
			assert!(disk.active, "disk is new failure");
		},
	)
	.await
}

#[derive(QueryableByName, Debug)]
struct CatalogRow {
	#[diesel(sql_type = sql_types::Text)]
	severity: String,
	#[diesel(sql_type = sql_types::Bool)]
	pending_review: bool,
}

async fn fetch_catalog(
	conn: &mut diesel_async::AsyncPgConnection,
	check_name: &str,
) -> Option<CatalogRow> {
	sql_query(
		"SELECT severity, reviewed_at IS NULL AS pending_review \
		 FROM healthcheck_severities WHERE check_name = $1",
	)
	.bind::<sql_types::Text, _>(check_name)
	.get_result(conn)
	.await
	.ok()
}

/// A status push with a never-before-seen check name inserts a catalog
/// row at the default Warning severity, marked as pending review.
/// Applies to both failing and passing checks: operators should be able
/// to pre-configure severities before a check ever fails.
#[tokio::test(flavor = "multi_thread")]
async fn submit_status_seeds_catalog_for_new_checks() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			assert!(fetch_catalog(&mut conn, "brand_new_check").await.is_none());
			assert!(fetch_catalog(&mut conn, "passing_check").await.is_none());

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": true,
					"health": [
						{ "check": "brand_new_check", "healthy": false },
						{ "check": "passing_check", "healthy": true },
					],
				}),
			)
			.await;

			let failing = fetch_catalog(&mut conn, "brand_new_check")
				.await
				.expect("failing check seeded in catalog");
			assert_eq!(failing.severity, "warning");
			assert!(failing.pending_review);

			let passing = fetch_catalog(&mut conn, "passing_check")
				.await
				.expect("passing check seeded in catalog");
			assert_eq!(passing.severity, "warning");
			assert!(passing.pending_review);
		},
	)
	.await
}

/// Editing the catalog severity for a check changes the severity used
/// by subsequent status pushes' per-check issue filings.
#[tokio::test(flavor = "multi_thread")]
async fn submit_status_uses_catalog_severity_on_failure() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = insert_health_test_server(&mut conn, device_id).await;

			set_check_severity(&mut conn, "tunable_check", "critical").await;

			post_status(
				&public,
				&cert,
				server_id,
				serde_json::json!({
					"healthy": false,
					"health": [ { "check": "tunable_check", "healthy": false } ],
				}),
			)
			.await;

			let issue = fetch_issue(&mut conn, server_id, "status", "health/tunable_check")
				.await
				.expect("per-check issue filed");
			assert_eq!(issue.severity, "critical");
		},
	)
	.await
}
