//! Endpoint tests for the operator-facing `/api/backups/*` fns.
//!
//! The test harness uses the in-memory backup Secret store, so onboarding
//! (Canopy generating/storing the repo passphrase) is exercised without a
//! cluster.

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

/// Assert a group has no backup config — a rejected upsert must not partially
/// write. (A macro, not a fn, to avoid naming `axum_test::TestServer`.)
macro_rules! assert_no_config {
	($private:expr, $group:expr) => {{
		let resp = $private
			.post("/api/backups/get")
			.json(&serde_json::json!({ "server_group_id": $group }))
			.await;
		resp.assert_status_ok();
		assert!(
			resp.json::<serde_json::Value>().is_null(),
			"expected no config to be written"
		);
	}};
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

		// With a passphrase → created (no escrow; provisioning until init → ready).
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
async fn probe_reports_state_and_already_configured() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;

		// The harness uses the bucket-name-derived fake prober; "fresh" has no
		// marker, so it reads empty.
		let resp = private
			.post("/api/backups/probe")
			.json(&serde_json::json!({
				"bucket": "fresh",
				"prefix": "",
				"maintenance_role_arn": "maint",
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["state"], "empty");
		assert!(body["already_configured"].is_null());

		// A config for this bucket+prefix → the probe reports its group.
		private
			.post("/api/backups/create")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "taken",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
				"mode": "from_birth",
			}))
			.await
			.assert_status_ok();
		let resp = private
			.post("/api/backups/probe")
			.json(&serde_json::json!({
				"bucket": "taken",
				"prefix": "",
				"maintenance_role_arn": "maint",
			}))
			.await;
		resp.assert_status_ok();
		assert_eq!(
			resp.json::<serde_json::Value>()["already_configured"],
			group_id.to_string()
		);
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

#[tokio::test(flavor = "multi_thread")]
async fn upsert_creates_then_reapplies_idempotently() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;

		// First apply → creates from-birth (the fake prober reports empty),
		// provisions, and sets the schedule.
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-iac",
				"prefix": "p/",
				"target_role_arn": "arn:aws:iam::123:role/dev",
				"maintenance_role_arn": "arn:aws:iam::123:role/maint",
				"region": "ap-southeast-2",
				"expected_interval": 3600,
				"retention": retention_json(),
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["status"], "provisioning");
		assert_eq!(body["mode"], "from_birth");
		assert_eq!(body["maintenance_role_arn"], "arn:aws:iam::123:role/maint");
		let schedule = &body["schedules"][0];
		assert_eq!(schedule["type"], "tamanu-postgres");
		assert_eq!(schedule["expected_interval"], 3600);

		// Re-apply with changed mutable fields → updates in place, no duplicate.
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-iac",
				"prefix": "p/",
				"target_role_arn": "arn:aws:iam::123:role/dev2",
				"maintenance_role_arn": "arn:aws:iam::123:role/maint2",
				"region": "us-east-1",
				"expected_interval": null,
				"retention": retention_json(),
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["maintenance_role_arn"], "arn:aws:iam::123:role/maint2");
		assert_eq!(body["target_role_arn"], "arn:aws:iam::123:role/dev2");
		assert_eq!(body["region"], "us-east-1");
		// Manual-only now. (That this second apply returns 200 with updated
		// fields — rather than the 409 a duplicate create would — is what proves
		// it took the in-place update path, i.e. no duplicate row.)
		assert!(body["schedules"][0]["expected_interval"].is_null());

		// Changing the bucket (identity) is rejected.
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-iac-moved",
				"prefix": "p/",
				"target_role_arn": "arn:aws:iam::123:role/dev2",
				"maintenance_role_arn": "arn:aws:iam::123:role/maint2",
			}))
			.await;
		resp.assert_status_conflict();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_missing_group_is_404() {
	commons_tests::server::run(async |_conn, _public, private| {
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": Uuid::new_v4(),
				"bucket": "b",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint-arn",
			}))
			.await;
		resp.assert_status_not_found();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_rejects_below_floor_retention() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-iac",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint",
				"retention": {
					"keep_latest": 1, "keep_daily": 1, "keep_weekly": 1,
					"keep_monthly": 1, "keep_annual": 0
				},
			}))
			.await;
		resp.assert_status_bad_request();
		// Floor is checked before anything is written.
		assert_no_config!(private, group_id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_rejects_prefix_change() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		let base = serde_json::json!({
			"server_group_id": group_id,
			"bucket": "bes-iac",
			"prefix": "a/",
			"target_role_arn": "arn",
			"maintenance_role_arn": "maint",
		});
		private
			.post("/api/backups/upsert")
			.json(&base)
			.await
			.assert_status_ok();

		// Same bucket, different prefix → prefix is part of the immutable identity.
		let mut moved = base.clone();
		moved["prefix"] = serde_json::json!("b/");
		private
			.post("/api/backups/upsert")
			.json(&moved)
			.await
			.assert_status_conflict();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_rejects_existing_repo() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		// `…existing…` → the fake prober reports an existing kopia repo. The
		// machine API never imports; that's an interactive wizard action.
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-existing-repo",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint",
			}))
			.await;
		resp.assert_status_conflict();
		assert_no_config!(private, group_id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_rejects_other_content() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-other-stuff",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint",
			}))
			.await;
		resp.assert_status_conflict();
		assert_no_config!(private, group_id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_rejects_inaccessible_bucket() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_id = seed_group(&mut conn).await;
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"bucket": "bes-denied",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint",
			}))
			.await;
		// Inaccessible → AppError::Upstream → 502.
		assert_eq!(resp.status_code().as_u16(), 502);
		assert_no_config!(private, group_id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_rejects_bucket_already_configured_for_another_group() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let group_a = seed_group(&mut conn).await;
		let group_b = seed_group(&mut conn).await;

		private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_a,
				"bucket": "bes-shared",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint",
			}))
			.await
			.assert_status_ok();

		// A different group can't claim the same bucket/prefix.
		let resp = private
			.post("/api/backups/upsert")
			.json(&serde_json::json!({
				"server_group_id": group_b,
				"bucket": "bes-shared",
				"target_role_arn": "arn",
				"maintenance_role_arn": "maint",
			}))
			.await;
		resp.assert_status_conflict();
		assert_no_config!(private, group_b);
	})
	.await;
}

/// Decrypt an `age` ciphertext with the given identity (test-side, proving the
/// vault format is standard and the ceremony round-trips).
fn age_decrypt(ciphertext: &[u8], identity: &age::x25519::Identity) -> Vec<u8> {
	use std::io::Read;
	let decryptor = age::Decryptor::new_buffered(ciphertext).unwrap();
	let mut reader = decryptor
		.decrypt(std::iter::once(identity as &dyn age::Identity))
		.unwrap();
	let mut out = Vec::new();
	reader.read_to_end(&mut out).unwrap();
	out
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_ceremony_roundtrip() {
	use base64::prelude::*;

	// Each nextest test runs in its own process, so this env set is isolated.
	let identity = age::x25519::Identity::generate();
	unsafe {
		std::env::set_var(
			"CANOPY_RECOVERY_VAULT_KEYS",
			identity.to_public().to_string(),
		);
	}

	commons_tests::server::run(async move |_conn, _public, private| {
		// Status: configured but never verified → due.
		let resp = private
			.post("/api/backups/recovery_status")
			.json(&serde_json::json!({}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body["configured"], true);
		assert_eq!(body["due"], true);
		assert!(body["last_verified_at"].is_null());

		// Challenge → decrypt offline → verify.
		let resp = private
			.post("/api/backups/recovery_challenge")
			.json(&serde_json::json!({}))
			.await;
		resp.assert_status_ok();
		let ct_b64 = resp.json::<serde_json::Value>()["ciphertext_base64"]
			.as_str()
			.unwrap()
			.to_string();
		let plaintext = age_decrypt(&BASE64_STANDARD.decode(ct_b64).unwrap(), &identity);
		let answer = String::from_utf8(plaintext).unwrap();

		let resp = private
			.post("/api/backups/recovery_verify")
			.json(&serde_json::json!({ "answer": answer }))
			.await;
		resp.assert_status_ok();

		// Status now reports verified (not due) for the current recipients.
		let resp = private
			.post("/api/backups/recovery_status")
			.json(&serde_json::json!({}))
			.await;
		let body: serde_json::Value = resp.json();
		assert_eq!(body["due"], false);
		assert!(!body["last_verified_at"].is_null());
		assert_eq!(
			body["last_verified_recipients"][0],
			identity.to_public().to_string()
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_verify_rejects_wrong_and_missing() {
	let identity = age::x25519::Identity::generate();
	unsafe {
		std::env::set_var(
			"CANOPY_RECOVERY_VAULT_KEYS",
			identity.to_public().to_string(),
		);
	}

	commons_tests::server::run(async move |_conn, _public, private| {
		// No outstanding challenge → 400.
		private
			.post("/api/backups/recovery_verify")
			.json(&serde_json::json!({ "answer": "anything" }))
			.await
			.assert_status_bad_request();

		// Issue a challenge, then answer wrong → 400 (and the challenge is spent).
		private
			.post("/api/backups/recovery_challenge")
			.json(&serde_json::json!({}))
			.await
			.assert_status_ok();
		private
			.post("/api/backups/recovery_verify")
			.json(&serde_json::json!({ "answer": "not-the-nonce" }))
			.await
			.assert_status_bad_request();
		// Spent: a second verify with no fresh challenge → 400.
		private
			.post("/api/backups/recovery_verify")
			.json(&serde_json::json!({ "answer": "not-the-nonce" }))
			.await
			.assert_status_bad_request();
	})
	.await;
}
