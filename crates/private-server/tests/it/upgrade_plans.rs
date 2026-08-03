//! Recording where a deployment is going, through the operator API, and reading
//! the fleet view that surfaces it.

use commons_tests::diesel_async::SimpleAsyncConnection;
use serde_json::{Value, json};

const GROUP: &str = "cccccccc-0000-0000-0000-000000000001";
const UNPLANNED: &str = "cccccccc-0000-0000-0000-000000000002";

#[tokio::test(flavor = "multi_thread")]
async fn record_then_the_fleet_view_shows_it() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
				('cccccccc-0000-0000-0000-0000000000f1', 2, 61, 0, 'x', 'published'),
				('cccccccc-0000-0000-0000-0000000000f2', 2, 63, 0, 'x', 'published');
			INSERT INTO server_groups (id, name, effective_version) VALUES
				('cccccccc-0000-0000-0000-000000000001', 'kamaka', '2.60.0'),
				('cccccccc-0000-0000-0000-000000000002', 'no-plan', '2.60.0');",
		)
		.await
		.unwrap();

		// Planning the older minor deliberately: the site can only absorb 2.61.
		let resp = private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f1",
				// Long past, so `late` is stable however far in the future this
				// test runs. Date arithmetic itself is unit-tested with injected days.
				"planned_for": "2020-01-01",
				"note": "site can absorb 2.61 only",
			}))
			.await;
		resp.assert_status_ok();
		let plan: Value = resp.json();
		assert_eq!(plan["planned_for"], "2020-01-01");
		assert_eq!(plan["note"], "site can absorb 2.61 only");

		let fleet: Vec<Value> = private
			.post("/api/upgrade_plans/fleet")
			.json(&json!({}))
			.await
			.json();

		let planned = fleet
			.iter()
			.find(|row| row["group_id"] == GROUP)
			.expect("the planned group");
		assert_eq!(planned["target_version"], "2.61.0");
		assert_eq!(planned["current_version"], "2.60.0");
		assert_eq!(planned["late"], true, "the planned day has passed unmet");

		// A group with no plan is still listed: an unplanned deployment several
		// minors behind is what the view exists to surface.
		let unplanned = fleet
			.iter()
			.find(|row| row["group_id"] == UNPLANNED)
			.expect("the unplanned group");
		assert!(unplanned["plan"].is_null());
		assert_eq!(unplanned["late"], false);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_target_behind_the_group_is_refused() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
				('cccccccc-0000-0000-0000-0000000000f1', 2, 61, 0, 'x', 'published');
			INSERT INTO server_groups (id, name, effective_version) VALUES
				('cccccccc-0000-0000-0000-000000000001', 'ahead', '2.62.0');",
		)
		.await
		.unwrap();

		private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f1",
			}))
			.await
			.assert_status_bad_request();
	})
	.await;
}

/// A restore takes hours, so a group mid-test must not read as untested: an
/// issuance with no report yet is an attempt in flight.
#[tokio::test(flavor = "multi_thread")]
async fn an_attempt_in_flight_shows_beside_the_verdict() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
				('cccccccc-0000-0000-0000-0000000000f1', 2, 61, 0, 'x', 'published');
			INSERT INTO server_groups (id, name, effective_version) VALUES
				('cccccccc-0000-0000-0000-000000000001', 'kamaka', '2.60.0');
			INSERT INTO devices (id, role) VALUES
				('cccccccc-0000-0000-0000-0000000000d0', 'backup-restore');
			INSERT INTO upgrade_plans (group_id, target_version_id) VALUES
				('cccccccc-0000-0000-0000-000000000001',
				 'cccccccc-0000-0000-0000-0000000000f1');",
		)
		.await
		.unwrap();

		let before: Vec<Value> = private
			.post("/api/upgrade_plans/fleet")
			.json(&json!({}))
			.await
			.json();
		let row = before.iter().find(|r| r["group_id"] == GROUP).unwrap();
		assert!(row["attempt"].is_null(), "nothing has started");

		// Credentials issued, still valid, nothing reported: a restore is running.
		conn.batch_execute(
			"INSERT INTO backup_credential_issuances
				(device_id, group_id, type, purpose, issued_at, expires_at,
				 sts_assumed_role, bucket, prefix)
			 VALUES ('cccccccc-0000-0000-0000-0000000000d0',
				'cccccccc-0000-0000-0000-000000000001', 'tamanu-postgres', 'restore',
				NOW() - INTERVAL '20 minutes', NOW() + INTERVAL '40 minutes',
				'arn:aws:iam::1:role/r', 'b', '')",
		)
		.await
		.unwrap();

		let during: Vec<Value> = private
			.post("/api/upgrade_plans/fleet")
			.json(&json!({}))
			.await
			.json();
		let row = during.iter().find(|r| r["group_id"] == GROUP).unwrap();
		assert_eq!(
			row["attempt"], "in_flight",
			"a group mid-test does not read as merely untested"
		);
		assert_eq!(
			row["verdict"], "nottested",
			"and the verdict is untouched by the activity"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn amend_changes_the_date_and_note_without_replacing_the_plan() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
				('cccccccc-0000-0000-0000-0000000000f1', 2, 61, 0, 'x', 'published');
			INSERT INTO server_groups (id, name, effective_version) VALUES
				('cccccccc-0000-0000-0000-000000000001', 'kamaka', '2.60.0');",
		)
		.await
		.unwrap();

		let recorded: Value = private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f1",
				"planned_for": "2020-01-01",
				"note": "waiting on the site",
			}))
			.await
			.json();

		let resp = private
			.post("/api/upgrade_plans/amend")
			.json(&json!({
				"id": recorded["id"],
				"planned_for": "2020-02-02",
				"note": "site confirmed the window",
			}))
			.await;
		resp.assert_status_ok();
		let amended: Value = resp.json();
		assert_eq!(amended["id"], recorded["id"], "the same plan");
		assert_eq!(amended["planned_for"], "2020-02-02");
		assert_eq!(amended["note"], "site confirmed the window");
		assert_eq!(
			amended["target_version_id"], recorded["target_version_id"],
			"amending does not move the deployment somewhere else"
		);
		assert!(amended["superseded_at"].is_null());
		assert!(!amended["amended_at"].is_null());

		let fleet: Vec<Value> = private
			.post("/api/upgrade_plans/fleet")
			.json(&json!({}))
			.await
			.json();
		let row = fleet
			.iter()
			.find(|row| row["group_id"] == GROUP)
			.expect("the planned group");
		assert_eq!(row["plan"]["note"], "site confirmed the window");
		assert_eq!(
			row["target_version"], "2.61.0",
			"still going to the same place"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn amending_a_withdrawn_plan_is_refused() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
				('cccccccc-0000-0000-0000-0000000000f1', 2, 61, 0, 'x', 'published'),
				('cccccccc-0000-0000-0000-0000000000f2', 2, 63, 0, 'x', 'published');
			INSERT INTO server_groups (id, name, effective_version) VALUES
				('cccccccc-0000-0000-0000-000000000001', 'kamaka', '2.60.0');",
		)
		.await
		.unwrap();

		let first: Value = private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f1",
			}))
			.await
			.json();

		// Recording a second plan replaces the first, which is then history.
		private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f2",
			}))
			.await
			.assert_status_ok();

		private
			.post("/api/upgrade_plans/amend")
			.json(&json!({ "id": first["id"], "note": "too late" }))
			.await
			.assert_status_bad_request();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_history_view_shows_a_withdrawn_plan_beside_a_replaced_one() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
				('cccccccc-0000-0000-0000-0000000000f1', 2, 61, 0, 'x', 'published'),
				('cccccccc-0000-0000-0000-0000000000f2', 2, 63, 0, 'x', 'published');
			INSERT INTO server_groups (id, name, effective_version) VALUES
				('cccccccc-0000-0000-0000-000000000001', 'kamaka', '2.60.0');",
		)
		.await
		.unwrap();

		let replaced: Value = private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f1",
				"planned_for": "2020-01-01",
				"note": "site can absorb 2.61 only",
			}))
			.await
			.json();

		let withdrawn: Value = private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f2",
			}))
			.await
			.json();
		private
			.post("/api/upgrade_plans/withdraw")
			.json(&json!({ "id": withdrawn["id"] }))
			.await
			.assert_status_ok();

		let history: Vec<Value> = private
			.post("/api/upgrade_plans/history")
			.json(&json!({}))
			.await
			.json();
		assert_eq!(history.len(), 2, "both closed plans are readable");

		// Most recently closed first: the withdrawal happened after the
		// replacement it followed.
		assert_eq!(history[0]["plan"]["id"], withdrawn["id"]);
		assert_eq!(history[0]["outcome"], "withdrawn");
		assert_eq!(history[0]["target_version"], "2.63.0");
		assert_eq!(history[0]["group_name"], "kamaka");
		assert!(
			!history[0]["plan"]["withdrawn_by"].is_null(),
			"who withdrew it is part of the record"
		);
		assert_eq!(history[0]["ended_at"], history[0]["plan"]["withdrawn_at"]);

		assert_eq!(history[1]["plan"]["id"], replaced["id"]);
		assert_eq!(history[1]["outcome"], "replaced");
		assert_eq!(history[1]["plan"]["planned_for"], "2020-01-01");
		assert_eq!(history[1]["plan"]["note"], "site can absorb 2.61 only");

		// An open plan is not history.
		private
			.post("/api/upgrade_plans/record")
			.json(&json!({
				"group_id": GROUP,
				"target_version_id": "cccccccc-0000-0000-0000-0000000000f1",
			}))
			.await
			.assert_status_ok();
		let history: Vec<Value> = private
			.post("/api/upgrade_plans/history")
			.json(&json!({}))
			.await
			.json();
		assert_eq!(history.len(), 2);
	})
	.await;
}
