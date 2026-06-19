//! HTTP tests for the device backup endpoints. The 412/409/502 resolution
//! matrix runs against the standard harness (where `AppState.sts`/`kube` are
//! `None`); the successful-issuance (200) path builds a public-server with a
//! mocked STS client injected.

use aws_sdk_sts::operation::assume_role::AssumeRoleOutput;
use aws_sdk_sts::types::Credentials;
use aws_smithy_mocks::{RuleMode, mock};
use axum_client_ip::ClientIpSource;
use commons_servers::router;
use commons_tests::axum_test::TestServer;
use diesel::{sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

// --- seeding helpers --------------------------------------------------------

async fn make_group(conn: &mut AsyncPgConnection) -> Uuid {
	let id = Uuid::new_v4();
	sql_query("INSERT INTO server_groups (id, name) VALUES ($1, 'backup-test-group')")
		.bind::<sql_types::Uuid, _>(id)
		.execute(conn)
		.await
		.expect("insert group");
	id
}

/// Create a live server bound to `device_id`, optionally in `group_id`.
async fn make_server(
	conn: &mut AsyncPgConnection,
	device_id: Uuid,
	group_id: Option<Uuid>,
) -> Uuid {
	let server_id = Uuid::new_v4();
	sql_query(
		"INSERT INTO servers (id, host, kind, device_id, group_id) \
		 VALUES ($1, 'https://srv.example.com', 'central', $2, $3)",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Uuid, _>(device_id)
	.bind::<sql_types::Nullable<sql_types::Uuid>, _>(group_id)
	.execute(conn)
	.await
	.expect("insert server");
	server_id
}

async fn make_config(conn: &mut AsyncPgConnection, group_id: Uuid, status: &str) {
	sql_query(
		"INSERT INTO server_group_backup_config \
		 (group_id, bucket, prefix, target_role_arn, region, repo_password_ref, status) \
		 VALUES ($1, 'grp-bucket', '', 'arn:aws:iam::123456789012:role/grp', 'ap-southeast-2', 'grp-repo-pw', $2)",
	)
	.bind::<sql_types::Uuid, _>(group_id)
	.bind::<sql_types::Text, _>(status)
	.execute(conn)
	.await
	.expect("insert config");
}

async fn enable_capability(conn: &mut AsyncPgConnection, server_id: Uuid, r#type: &str) {
	sql_query(
		"INSERT INTO server_backup_capabilities (server_id, type, enabled) VALUES ($1, $2, true)",
	)
	.bind::<sql_types::Uuid, _>(server_id)
	.bind::<sql_types::Text, _>(r#type)
	.execute(conn)
	.await
	.expect("insert capability");
}

// --- 412 / 409 / 502 resolution matrix --------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn credentials_no_live_server_is_412() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |_conn, cert, _device_id, public, _| {
			// No server row seeded for this device.
			let resp = public
				.post("/backup-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::PRECONDITION_FAILED);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_ungrouped_server_is_409() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			make_server(&mut conn, device_id, None).await;
			let resp = public
				.post("/backup-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::CONFLICT);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_no_config_is_409() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_server(&mut conn, device_id, Some(group)).await;
			let resp = public
				.post("/backup-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::CONFLICT);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_dormant_config_is_409() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			let server = make_server(&mut conn, device_id, Some(group)).await;
			make_config(&mut conn, group, "provisioning").await;
			enable_capability(&mut conn, server, "tamanu-postgres").await;
			let resp = public
				.post("/backup-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::CONFLICT);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_type_not_enabled_is_409() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_server(&mut conn, device_id, Some(group)).await;
			make_config(&mut conn, group, "ready").await;
			// No capability row → not enabled.
			let resp = public
				.post("/backup-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::CONFLICT);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_ready_but_sts_unconfigured_is_502() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			let server = make_server(&mut conn, device_id, Some(group)).await;
			make_config(&mut conn, group, "ready").await;
			enable_capability(&mut conn, server, "tamanu-postgres").await;
			// Harness default: AppState.sts == None → 502.
			let resp = public
				.post("/backup-credentials")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "type": "tamanu-postgres" }))
				.await;
			resp.assert_status(http::StatusCode::BAD_GATEWAY);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn target_no_live_server_is_412() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |_conn, cert, _device_id, public, _| {
			let resp = public
				.get("/backup-target")
				.add_header("mtls-certificate", &cert)
				.await;
			resp.assert_status(http::StatusCode::PRECONDITION_FAILED);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn target_ready_but_kube_unconfigured_is_502() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_server(&mut conn, device_id, Some(group)).await;
			make_config(&mut conn, group, "ready").await;
			// Harness default: AppState.kube == None → 502.
			let resp = public
				.get("/backup-target")
				.add_header("mtls-certificate", &cert)
				.await;
			resp.assert_status(http::StatusCode::BAD_GATEWAY);
		},
	)
	.await;
}

// --- capabilities -----------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn capabilities_registers_and_204() {
	use commons_types::backup::BackupType;
	use database::ServerBackupCapability;

	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			let server = make_server(&mut conn, device_id, Some(group)).await;

			let resp = public
				.post("/backup-capabilities")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "types": ["tamanu-postgres", "custom-thing"] }))
				.await;
			resp.assert_status(http::StatusCode::NO_CONTENT);

			let caps = ServerBackupCapability::list_for_server(&mut conn, server)
				.await
				.unwrap();
			assert_eq!(caps.len(), 2);
			// No backup_type_defaults seeded → both default to disabled.
			assert!(caps.iter().all(|c| !c.enabled));
			assert!(caps.iter().any(|c| c.r#type == BackupType::TamanuPostgres));
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn capabilities_ungrouped_is_409() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			make_server(&mut conn, device_id, None).await;
			let resp = public
				.post("/backup-capabilities")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "types": ["tamanu-postgres"] }))
				.await;
			resp.assert_status(http::StatusCode::CONFLICT);
		},
	)
	.await;
}

// --- report -----------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn report_writes_run_with_context_attribution_and_204() {
	use database::BackupRun;

	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			let server = make_server(&mut conn, device_id, Some(group)).await;
			let run_id = Uuid::new_v4();

			// Send a bogus group_id in the body — it must be ignored; the
			// server derives group/device from the authenticated context.
			let bogus_group = Uuid::new_v4();
			let resp = public
				.post("/backup-report")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"run_id": run_id,
					"type": "tamanu-postgres",
					"purpose": "backup",
					"outcome": "success",
					"group_id": bogus_group,
					"bytes_uploaded": 4096,
					"snapshot_id": "k0pia123",
				}))
				.await;
			resp.assert_status(http::StatusCode::NO_CONTENT);

			let runs = BackupRun::list_for_group(&mut conn, group, 10)
				.await
				.unwrap();
			assert_eq!(runs.len(), 1);
			let run = &runs[0];
			assert_eq!(run.id, run_id);
			assert_eq!(
				run.group_id, group,
				"group must come from context, not body"
			);
			assert_eq!(run.device_id, device_id);
			assert_eq!(run.server_id, Some(server));
			assert_eq!(run.bytes_uploaded, Some(4096));
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn report_duplicate_run_id_is_409() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group = make_group(&mut conn).await;
			make_server(&mut conn, device_id, Some(group)).await;
			let run_id = Uuid::new_v4();
			let body = serde_json::json!({
				"run_id": run_id,
				"type": "tamanu-postgres",
				"purpose": "backup",
				"outcome": "success",
			});

			let first = public
				.post("/backup-report")
				.add_header("mtls-certificate", &cert)
				.json(&body)
				.await;
			first.assert_status(http::StatusCode::NO_CONTENT);

			let dup = public
				.post("/backup-report")
				.add_header("mtls-certificate", &cert)
				.json(&body)
				.await;
			dup.assert_status(http::StatusCode::CONFLICT);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn report_ungrouped_is_409() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			make_server(&mut conn, device_id, None).await;
			let resp = public
				.post("/backup-report")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"run_id": Uuid::new_v4(),
					"type": "tamanu-postgres",
					"purpose": "backup",
					"outcome": "failure",
					"error": "kopia exploded",
				}))
				.await;
			resp.assert_status(http::StatusCode::CONFLICT);
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn report_no_live_server_is_412() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |_conn, cert, _device_id, public, _| {
			let resp = public
				.post("/backup-report")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"run_id": Uuid::new_v4(),
					"type": "tamanu-postgres",
					"purpose": "backup",
					"outcome": "success",
				}))
				.await;
			resp.assert_status(http::StatusCode::PRECONDITION_FAILED);
		},
	)
	.await;
}

// --- successful issuance (200) with a mocked STS client ---------------------

/// Build a public-server TestServer whose `AppState` carries the given STS
/// client (and no kube), against the given DB url. Mirrors the harness wiring
/// but lets the test inject the stubbed STS path.
fn public_server_with_sts(url: &str, sts: aws_sdk_sts::Client) -> TestServer {
	let state = public_server::state::AppState {
		db: database::init_to(url),
		tera: public_server::state::AppState::init_tera().unwrap(),
		server_versions_secret: Some("test-secret".to_string()),
		tailnet_directory: None,
		rate_limiter: Default::default(),
		sts: Some(sts),
		kube: None,
	};
	let app = router(
		axum::Router::from(public_server::routes().with_state(state)),
		ClientIpSource::RightmostForwarded,
	);
	let mut server = TestServer::new(app);
	server.add_header("Forwarded", "for=192.0.1.60");
	server.add_header("X-Version", "3.4.5");
	server
}

async fn seed_device(conn: &mut AsyncPgConnection, role: &str) -> (Uuid, String) {
	let (key_data, cert) = commons_tests::server::make_certificate();
	#[derive(diesel::QueryableByName)]
	struct Row {
		#[diesel(sql_type = sql_types::Uuid)]
		id: Uuid,
	}
	let row: Row = sql_query("INSERT INTO devices (role) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(role)
		.get_result(conn)
		.await
		.expect("insert device");
	sql_query(
		"INSERT INTO device_keys (device_id, key_data, name, is_active) \
		 VALUES ($1, $2, 'Test Key', true)",
	)
	.bind::<sql_types::Uuid, _>(row.id)
	.bind::<sql_types::Binary, _>(key_data)
	.execute(conn)
	.await
	.expect("insert device key");
	(row.id, cert)
}

fn assume_role_rule(policy_expectation: Option<&'static str>) -> aws_smithy_mocks::Rule {
	mock!(aws_sdk_sts::Client::assume_role)
		.match_requests(move |req| match policy_expectation {
			// Backup: no session policy.
			None => req.policy().is_none(),
			// Restore: the read-only session policy ANDs down access.
			Some(needle) => req.policy().map(|p| p.contains(needle)).unwrap_or(false),
		})
		.then_output(|| {
			AssumeRoleOutput::builder()
				.credentials(
					Credentials::builder()
						.access_key_id("AKIATESTKEY")
						.secret_access_key("test-secret-key")
						.session_token("test-session-token")
						.expiration(aws_sdk_sts::primitives::DateTime::from_secs(1_900_000_000))
						.build()
						.unwrap(),
				)
				.build()
		})
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_backup_happy_path_200_and_audit() {
	use commons_types::backup::BackupPurpose;
	use database::BackupCredentialIssuance;

	commons_tests::db::TestDb::run(async |mut conn, url| {
		let (device_id, cert) = seed_device(&mut conn, "server").await;
		let group = make_group(&mut conn).await;
		let server = make_server(&mut conn, device_id, Some(group)).await;
		make_config(&mut conn, group, "ready").await;
		enable_capability(&mut conn, server, "tamanu-postgres").await;

		let rule = assume_role_rule(None);
		let sts = aws_smithy_mocks::mock_client!(aws_sdk_sts, RuleMode::MatchAny, [&rule]);
		let public = public_server_with_sts(&url, sts);

		let resp = public
			.post("/backup-credentials")
			.add_header("mtls-certificate", &cert)
			.json(&serde_json::json!({ "type": "tamanu-postgres", "purpose": "backup" }))
			.await;
		resp.assert_status_ok();

		let body: serde_json::Value = resp.json();
		assert_eq!(body["Version"], 1);
		assert_eq!(body["AccessKeyId"], "AKIATESTKEY");
		assert_eq!(body["SecretAccessKey"], "test-secret-key");
		assert_eq!(body["SessionToken"], "test-session-token");
		assert!(body["Expiration"].as_str().unwrap().ends_with('Z'));

		// Audit row recorded with the assumed role + access key id.
		let issuances = BackupCredentialIssuance::list_for_group(&mut conn, group, 10)
			.await
			.unwrap();
		assert_eq!(issuances.len(), 1);
		let iss = &issuances[0];
		assert_eq!(iss.purpose, BackupPurpose::Backup);
		assert_eq!(iss.access_key_id.as_deref(), Some("AKIATESTKEY"));
		assert_eq!(iss.sts_assumed_role, "arn:aws:iam::123456789012:role/grp");
		assert_eq!(iss.bucket, "grp-bucket");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_restore_sends_session_policy() {
	use commons_types::backup::BackupPurpose;
	use database::BackupCredentialIssuance;

	commons_tests::db::TestDb::run(async |mut conn, url| {
		let (device_id, cert) = seed_device(&mut conn, "server").await;
		let group = make_group(&mut conn).await;
		let server = make_server(&mut conn, device_id, Some(group)).await;
		make_config(&mut conn, group, "ready").await;
		enable_capability(&mut conn, server, "tamanu-postgres").await;

		// The rule only matches if a session policy naming the bucket was sent.
		let rule = assume_role_rule(Some("arn:aws:s3:::grp-bucket"));
		let sts = aws_smithy_mocks::mock_client!(aws_sdk_sts, RuleMode::MatchAny, [&rule]);
		let public = public_server_with_sts(&url, sts);

		let resp = public
			.post("/backup-credentials")
			.add_header("mtls-certificate", &cert)
			.json(&serde_json::json!({ "type": "tamanu-postgres", "purpose": "restore" }))
			.await;
		resp.assert_status_ok();

		let issuances = BackupCredentialIssuance::list_for_group(&mut conn, group, 10)
			.await
			.unwrap();
		assert_eq!(issuances[0].purpose, BackupPurpose::Restore);
	})
	.await;
}
