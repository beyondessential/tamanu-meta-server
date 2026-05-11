use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
#[allow(dead_code)]
struct IssueRow {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
	#[diesel(sql_type = sql_types::Uuid)]
	server_id: Uuid,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
	device_id: Option<Uuid>,
	#[diesel(sql_type = sql_types::Text)]
	source: String,
	#[diesel(sql_type = sql_types::Text)]
	severity: String,
	#[diesel(sql_type = sql_types::Text)]
	message: String,
	#[diesel(sql_type = sql_types::Bool)]
	active: bool,
}

async fn provision_server(
	conn: &mut diesel_async::AsyncPgConnection,
	device_id: Uuid,
) -> Uuid {
	let server_id = Uuid::new_v4();
	sql_query(
		"INSERT INTO servers (id, host, kind, device_id) \
		 VALUES ($1, 'https://test.example.com', 'central', $2)",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Uuid, _>(device_id)
	.execute(conn)
	.await
	.expect("insert server");
	server_id
}

async fn issue_by_id(conn: &mut diesel_async::AsyncPgConnection, id: Uuid) -> IssueRow {
	sql_query(
		"SELECT id, server_id, device_id, source, severity, message, active \
		 FROM issues WHERE id = $1",
	)
	.bind::<sql_types::Uuid, _>(id)
	.get_result(conn)
	.await
	.expect("fetch issue")
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_creates_issue() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = provision_server(&mut conn, device_id).await;

			let response = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "disk-/var",
					"message": "less than 5% free",
				}))
				.await;
			response.assert_status_ok();

			let body: serde_json::Value = response.json();
			let issue_id = Uuid::parse_str(body.get("id").unwrap().as_str().unwrap()).unwrap();
			assert_eq!(
				body.get("server_id").and_then(|v| v.as_str()),
				Some(server_id.to_string().as_str())
			);
			assert_eq!(
				body.get("severity").and_then(|v| v.as_str()),
				Some("error") // default
			);
			assert_eq!(body.get("active").and_then(|v| v.as_bool()), Some(true));

			let row = issue_by_id(&mut conn, issue_id).await;
			assert_eq!(row.server_id, server_id);
			assert_eq!(row.device_id, Some(device_id));
			assert_eq!(row.source, "watchdog");
			assert_eq!(row.severity, "error");
			assert_eq!(row.message, "less than 5% free");
			assert!(row.active);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_dedups_by_ref() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = provision_server(&mut conn, device_id).await;

			let first = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "disk-/var",
					"message": "less than 5% free",
				}))
				.await;
			first.assert_status_ok();
			let first_id = first.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();

			let second = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "disk-/var",
					"severity": "critical",
					"message": "less than 1% free",
				}))
				.await;
			second.assert_status_ok();
			let second_id = second.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();

			assert_eq!(first_id, second_id, "same (server, source, ref) should be one issue");

			let row = issue_by_id(&mut conn, Uuid::parse_str(&first_id).unwrap()).await;
			assert_eq!(row.server_id, server_id);
			assert_eq!(row.severity, "critical");
			assert_eq!(row.message, "less than 1% free");

			#[derive(QueryableByName)]
			struct Counts {
				#[diesel(sql_type = sql_types::BigInt)]
				event_count: i64,
			}
			let counts: Counts = sql_query("SELECT COUNT(*) AS event_count FROM events WHERE issue_id = $1")
				.bind::<sql_types::Uuid, _>(Uuid::parse_str(&first_id).unwrap())
				.get_result(&mut conn)
				.await
				.expect("count events");
			assert_eq!(counts.event_count, 2, "different content → two event rows");
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_coalesces_identical_pushes() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			provision_server(&mut conn, device_id).await;

			// Three identical pushes.
			let mut issue_id = String::new();
			for _ in 0..3 {
				let r = public
					.post("/events")
					.add_header("mtls-certificate", &cert)
					.json(&serde_json::json!({
						"source": "watchdog",
						"ref": "x",
						"message": "same content",
					}))
					.await;
				r.assert_status_ok();
				issue_id = r.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();
			}

			#[derive(QueryableByName)]
			struct Counts {
				#[diesel(sql_type = sql_types::BigInt)]
				event_count: i64,
				#[diesel(sql_type = sql_types::Integer)]
				latest_occurrences: i32,
			}
			let counts: Counts = sql_query(
				"SELECT \
					(SELECT COUNT(*) FROM events WHERE issue_id = $1) AS event_count, \
					(SELECT occurrences FROM events WHERE issue_id = $1 ORDER BY created_at DESC LIMIT 1) AS latest_occurrences",
			)
			.bind::<sql_types::Uuid, _>(Uuid::parse_str(&issue_id).unwrap())
			.get_result(&mut conn)
			.await
			.expect("count");
			assert_eq!(counts.event_count, 1, "identical pushes collapse into one row");
			assert_eq!(counts.latest_occurrences, 3);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_active_false_resolves_issue() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			provision_server(&mut conn, device_id).await;

			let opened = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"message": "trouble",
				}))
				.await;
			opened.assert_status_ok();
			let issue_id = opened.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();

			let resolved = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"active": false,
					"message": "trouble",
				}))
				.await;
			resolved.assert_status_ok();
			assert_eq!(
				resolved.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap(),
				issue_id
			);

			let row = issue_by_id(&mut conn, Uuid::parse_str(&issue_id).unwrap()).await;
			assert!(!row.active, "issue should be inactive after active:false");
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_rejects_manual_source() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			provision_server(&mut conn, device_id).await;
			let response = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "manual",
					"ref": "x",
					"message": "nope",
				}))
				.await;
			assert_eq!(response.status_code().as_u16(), 400);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_rejects_device_with_no_server() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |_conn, cert, _device_id, public, _| {
			// No server provisioned for this device.
			let response = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"message": "should fail",
				}))
				.await;
			assert_eq!(response.status_code().as_u16(), 412);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_rejects_invalid_severity() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			provision_server(&mut conn, device_id).await;
			let response = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"severity": "kaboom",
					"message": "should fail",
				}))
				.await;
			assert!(
				!response.status_code().is_success(),
				"expected non-success, got {}",
				response.status_code()
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_opens_incident_at_error() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = provision_server(&mut conn, device_id).await;

			let r = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"severity": "error",
					"message": "trouble",
				}))
				.await;
			r.assert_status_ok();

			#[derive(QueryableByName)]
			struct IncidentCheck {
				#[diesel(sql_type = sql_types::Uuid)]
				id: Uuid,
				#[diesel(sql_type = sql_types::Uuid)]
				server_id: Uuid,
				#[diesel(sql_type = sql_types::Bool)]
				is_open: bool,
			}
			let inc: IncidentCheck = sql_query(
				"SELECT id, server_id, closed_at IS NULL AS is_open FROM incidents WHERE server_id = $1",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("one incident");
			assert_eq!(inc.server_id, server_id, "incident should be on this server (no parent)");
			assert!(inc.is_open, "incident should still be open");

			#[derive(QueryableByName)]
			struct LinkCheck {
				#[diesel(sql_type = sql_types::BigInt)]
				count: i64,
			}
			let links: LinkCheck = sql_query(
				"SELECT COUNT(*) AS count FROM incident_issues WHERE incident_id = $1 AND left_at IS NULL",
			)
			.bind::<sql_types::Uuid, _>(inc.id)
			.get_result(&mut conn)
			.await
			.expect("count");
			assert_eq!(links.count, 1);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_event_at_warning_does_not_open_incident() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = provision_server(&mut conn, device_id).await;

			let r = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"severity": "warning",
					"message": "minor",
				}))
				.await;
			r.assert_status_ok();

			#[derive(QueryableByName)]
			struct Counts {
				#[diesel(sql_type = sql_types::BigInt)]
				count: i64,
			}
			let counts: Counts = sql_query("SELECT COUNT(*) AS count FROM incidents WHERE server_id = $1")
				.bind::<sql_types::Uuid, _>(server_id)
				.get_result(&mut conn)
				.await
				.expect("count");
			assert_eq!(counts.count, 0, "warning shouldn't open an incident");
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn incident_closes_when_last_issue_resolves() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = provision_server(&mut conn, device_id).await;

			public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"severity": "error",
					"message": "trouble",
				}))
				.await
				.assert_status_ok();

			public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"severity": "error",
					"active": false,
					"message": "resolved",
				}))
				.await
				.assert_status_ok();

			#[derive(QueryableByName)]
			struct IncidentCheck {
				#[diesel(sql_type = sql_types::Bool)]
				is_closed: bool,
			}
			let inc: IncidentCheck = sql_query(
				"SELECT closed_at IS NOT NULL AS is_closed FROM incidents WHERE server_id = $1",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("incident");
			assert!(inc.is_closed, "incident should be closed");
		},
	)
	.await
}
