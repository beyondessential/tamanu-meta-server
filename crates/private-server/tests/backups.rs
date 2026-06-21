//! Endpoint tests for the operator-facing `/api/backups/*` fns.
//!
//! The test harness uses the in-memory backup Secret store, so onboarding
//! (Canopy generating/storing the repo passphrase) and the escrow reveal are
//! exercised end-to-end without a cluster.

use commons_tests::diesel_async::SimpleAsyncConnection;
use uuid::Uuid;

/// Seed a server group; returns its id.
async fn seed_group(conn: &mut impl SimpleAsyncConnection) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{id}', 'grp-{id}');"
	))
	.await
	.expect("seed group");
	id
}

fn retention_json() -> serde_json::Value {
	serde_json::json!({
		"keep_latest": 1,
		"keep_daily": 7,
		"keep_weekly": 4,
		"keep_monthly": 6,
		"keep_annual": 0
	})
}

#[tokio::test(flavor = "multi_thread")]
async fn create_get_zero_state_and_full_view() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;

		// Zero-state: unconfigured group → 200 null.
		let resp = private
			.post("/api/backups/get")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_ok();
		assert!(resp.json::<serde_json::Value>().is_null());

		// Create.
		let resp = private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-kopia-test",
				"target_role_arn": "arn:aws:iam::123:role/x",
				"maintenance_role_arn": "arn:aws:iam::123:role/x-maint",
				"mode": "from_birth",
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["status"], "provisioning");
		assert_eq!(body["mode"], "from_birth");
		assert_eq!(body["bucket"], "bes-kopia-test");

		// Get full view now.
		let resp = private
			.post("/api/backups/get")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["status"], "provisioning");

		// Duplicate create → 409.
		let resp = private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "other",
				"target_role_arn": "arn:aws:iam::123:role/y",
				"maintenance_role_arn": "arn:aws:iam::123:role/y-maint",
				"mode": "from_birth",
			}))
			.await;
		resp.assert_status_conflict();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn create_missing_group_is_404() {
	commons_tests::server::run(async |_conn, _public, private| {
		let resp = private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": Uuid::new_v4(),
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await;
		resp.assert_status_not_found();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn set_schedule_floor_rejected_and_accepted() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();

		// Below floor → 400.
		let resp = private
			.post("/api/backups/set_schedule")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"type": "tamanu-postgres",
				"expected_interval": 3600,
				"retention": {
					"keep_latest": 1, "keep_daily": 1, "keep_weekly": 1,
					"keep_monthly": 1, "keep_annual": 0
				},
			}))
			.await;
		resp.assert_status_bad_request();

		// At/above floor → ok, and the schedule round-trips into the view.
		let resp = private
			.post("/api/backups/set_schedule")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"type": "tamanu-postgres",
				"expected_interval": 3600,
				"retention": retention_json(),
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		let sched = &body["schedules"][0];
		assert_eq!(sched["type"], "tamanu-postgres");
		assert_eq!(sched["expected_interval"], 3600);
		assert_eq!(sched["retention"]["keep_daily"], 7);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn manual_only_interval_is_null() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();
		let resp = private
			.post("/api/backups/set_schedule")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"type": "tamanu-postgres",
				"expected_interval": null,
				"retention": retention_json(),
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert!(body["schedules"][0]["expected_interval"].is_null());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn create_repo_clears_error_and_is_idempotent() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();

		// Simulate an init-Job failure stamped on the row.
		conn.batch_execute(&format!(
			"UPDATE server_group_backup_config SET last_init_error = 'boom' WHERE group_id = '{group_id}';"
		))
		.await
		.expect("set error");

		let resp = private
			.post("/api/backups/create_repo")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["status"], "provisioning");
		assert!(body["last_init_error"].is_null(), "error cleared on retry");

		// Idempotent second call.
		private
			.post("/api/backups/create_repo")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await
			.assert_status_ok();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn create_repo_on_ready_is_409() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();
		conn.batch_execute(&format!(
			"UPDATE server_group_backup_config SET status = 'ready' WHERE group_id = '{group_id}';"
		))
		.await
		.expect("ready");
		let resp = private
			.post("/api/backups/create_repo")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_conflict();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ack_escrow_only_from_escrow_pending() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();

		// From provisioning → 409.
		private
			.post("/api/backups/ack_escrow")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await
			.assert_status_conflict();

		// Move to escrow_pending then ack → ready + stamps.
		conn.batch_execute(&format!(
			"UPDATE server_group_backup_config SET status = 'escrow_pending' WHERE group_id = '{group_id}';"
		))
		.await
		.expect("escrow_pending");
		let resp = private
			.post("/api/backups/ack_escrow")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["status"], "ready");
		assert!(!body["escrow_acked_at"].is_null());
		assert_eq!(body["escrow_acked_by"], "admin@localhost");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reveal_escrow_409_when_not_escrow_pending() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();
		// status is provisioning → 409 before any Secret read is attempted.
		let resp = private
			.post("/api/backups/reveal_escrow")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_conflict();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn from_birth_create_generates_a_revealable_passphrase() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();

		// Move to escrow_pending so reveal is allowed, then reveal the
		// canopy-generated passphrase from the (in-memory) Secret store.
		conn.batch_execute(&format!(
			"UPDATE server_group_backup_config SET status = 'escrow_pending' WHERE group_id = '{group_id}';"
		))
		.await
		.expect("escrow_pending");
		let resp = private
			.post("/api/backups/reveal_escrow")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert!(
			!body["passphrase"].as_str().unwrap_or_default().is_empty(),
			"a non-empty generated passphrase is revealed"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn passphrase_mode_requires_a_passphrase() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		// No passphrase supplied → 400.
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "passphrase",
			}))
			.await
			.assert_status_bad_request();

		// With a passphrase → created (skips escrow; provisioning until init).
		let resp = private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "passphrase",
				"passphrase": "an-existing-repo-passphrase",
			}))
			.await;
		resp.assert_status_ok();
		assert_eq!(resp.json::<serde_json::Value>()["mode"], "passphrase");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn request_now_upserts_and_cancel_deletes() {
	commons_tests::server::run(async |mut conn, _public, private| {
		// Server in a group so we can read the pending request back via stats.
		let group_id = seed_group(&mut conn).await;
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind, group_id) VALUES \
				('{server_id}', 'https://e.test', 'central', '{group_id}');"
		))
		.await
		.expect("seed server");

		let req = serde_json::json!({
			"server_id": server_id,
			"type": "tamanu-postgres",
			"purpose": "backup",
		});

		private
			.post("/api/backups/request_now")
			.json(&req)
			.await
			.assert_status_ok();
		// Re-request is a no-op upsert, not an error.
		private
			.post("/api/backups/request_now")
			.json(&req)
			.await
			.assert_status_ok();

		let stats = private
			.post("/api/backups/stats")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		stats.assert_status_ok();
		let body: serde_json::Value = stats.json();
		assert_eq!(
			body["pending_requests"].as_array().unwrap().len(),
			1,
			"single upserted row"
		);

		private
			.post("/api/backups/cancel_request")
			.json(&req)
			.await
			.assert_status_ok();
		let stats = private
			.post("/api/backups/stats")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		let body: serde_json::Value = stats.json();
		assert_eq!(
			body["pending_requests"].as_array().unwrap().len(),
			0,
			"cancel deleted the row"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stats_includes_runs_and_pending_requests() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		let device_id = Uuid::new_v4();
		let server_id = Uuid::new_v4();
		let run_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role) VALUES ('{device_id}', 'server');
			 INSERT INTO servers (id, host, kind, group_id, device_id) VALUES \
				('{server_id}', 'https://e.test', 'central', '{group_id}', '{device_id}');
			 INSERT INTO backup_repo_stats (group_id, snapshot_count, source_count, logical_bytes, physical_bytes) \
				VALUES ('{group_id}', 12, 3, 1000, 800);
			 INSERT INTO backup_runs (id, device_id, group_id, server_id, type, purpose, outcome, bytes_uploaded) \
				VALUES ('{run_id}', '{device_id}', '{group_id}', '{server_id}', 'tamanu-postgres', 'backup', 'success', 500);
			 INSERT INTO backup_requests (server_id, type, purpose) VALUES \
				('{server_id}', 'tamanu-postgres', 'backup');"
		))
		.await
		.expect("seed stats");

		let resp = private
			.post("/api/backups/stats")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["stats"]["snapshot_count"], 12);
		assert_eq!(body["recent_runs"].as_array().unwrap().len(), 1);
		assert_eq!(body["recent_runs"][0]["outcome"], "success");
		assert_eq!(body["pending_requests"].as_array().unwrap().len(), 1);
		assert_eq!(body["pending_requests"][0]["purpose"], "backup");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_region_and_delete() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"region": "ap-southeast-2",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();

		let resp = private
			.post("/api/backups/update")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"region": "us-east-1",
			}))
			.await;
		resp.assert_status_ok();
		assert_eq!(resp.json::<serde_json::Value>()["region"], "us-east-1");

		private
			.post("/api/backups/delete")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await
			.assert_status_ok();

		// Now a get → null again.
		let resp = private
			.post("/api/backups/get")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		resp.assert_status_ok();
		assert!(resp.json::<serde_json::Value>().is_null());
	})
	.await;
}
