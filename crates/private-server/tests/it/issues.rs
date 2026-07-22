use commons_tests::diesel_async::SimpleAsyncConnection;
use commons_types::status::CheckResult;
use database::issues::{CheckFiling, FilingScope};
use uuid::Uuid;

/// File one canopy-determined check observation on a server through the
/// real filing path, the way the reachability/backup sweeps do.
async fn seed_filed_check(
	conn: &mut database::diesel_async::AsyncPgConnection,
	server_id: Uuid,
	check: &str,
	observed: CheckResult,
	escalates: bool,
	message: &str,
) -> database::issues::Issue {
	database::issues::file_check(
		conn,
		CheckFiling {
			source: database::statuses::CANOPY_SOURCE,
			scope: FilingScope::Server {
				server_id,
				device_id: None,
			},
			check,
			observed,
			title: None,
			message,
			detail: None,
			default_ceiling: CheckResult::Failed,
			default_escalates: escalates,
			documentation: None,
		},
	)
	.await
	.expect("file check")
}

#[tokio::test(flavor = "multi_thread")]
async fn list_issues_for_device_and_server() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let device_id = Uuid::new_v4();
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role) VALUES ('{device_id}', 'server');
			 INSERT INTO device_keys (device_id, key_data, name, is_active) VALUES \
				('{device_id}', '\\x6b6579'::bytea, 'k', true);
			 INSERT INTO servers (id, host, kind, device_id) VALUES \
				('{server_id}', 'https://example.com', 'central', '{device_id}');
			 INSERT INTO issues (server_id, device_id, source, \"ref\", check_name, observed_result, effective_result, message, active, first_seen, last_seen, last_degraded_at) VALUES \
				('{server_id}', '{device_id}', 'src', 'a', 'a', 'failed',  'failed',  'newest', true,  '2026-05-03T10:00:00Z', '2026-05-03T10:00:00Z', '2026-05-03T10:00:00Z'),
				('{server_id}', '{device_id}', 'src', 'b', 'b', 'warning', 'warning', 'older',  true,  '2026-05-01T10:00:00Z', '2026-05-01T10:00:00Z', '2026-05-01T10:00:00Z'),
				('{server_id}', '{device_id}', 'src', 'c', 'c', 'passed',  'passed',  'gone',   false, '2026-05-02T10:00:00Z', '2026-05-02T10:00:00Z', '2026-05-02T10:00:00Z');"
		))
		.await
		.expect("seed");

		// Active-only (default).
		let resp = private
			.post("/api/issues/list_for_device")
			.json(&serde_json::json!({ "device_id": device_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 2, "default filter excludes inactive");
		assert_eq!(items[0].get("message").and_then(|v| v.as_str()), Some("newest"));
		assert_eq!(items[1].get("message").and_then(|v| v.as_str()), Some("older"));

		// Include resolved.
		let resp = private
			.post("/api/issues/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "active_only": false }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 3);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn incident_groups_at_server_group() {
	commons_tests::server::run(async |mut conn, _public, private| {
		// One group containing two equal-level servers.
		let device_id = Uuid::new_v4();
		let group_id = Uuid::new_v4();
		let server_a_id = Uuid::new_v4();
		let server_b_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'cluster');
			 INSERT INTO devices (id, role) VALUES ('{device_id}', 'server');
			 INSERT INTO device_keys (device_id, key_data, name, is_active) VALUES \
				('{device_id}', '\\x6b6579'::bytea, 'k', true);
			 INSERT INTO servers (id, host, kind, group_id) VALUES \
				('{server_a_id}', 'https://a.example.com', 'central', '{group_id}');
			 INSERT INTO servers (id, host, kind, device_id, group_id) VALUES \
				('{server_b_id}', 'https://b.example.com', 'facility', '{device_id}', '{group_id}');"
		))
		.await
		.expect("seed");

		// File a failing check on server B → opens an incident on the group.
		seed_filed_check(
			&mut conn,
			server_b_id,
			"x",
			CheckResult::Failed,
			false,
			"trouble in B",
		)
		.await;

		// Listing incidents by either member's id finds the same group-level incident.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_a_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "incident lives on the group");
		assert!(items[0].get("closed_at").map_or(true, |v| v.is_null()));
		let group_incident_id = items[0]
			.get("id")
			.and_then(|v| v.as_str())
			.unwrap()
			.to_string();

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_b_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(
			items.len(),
			1,
			"originating member resolves to the same group"
		);
		assert_eq!(
			items[0].get("id").and_then(|v| v.as_str()),
			Some(group_incident_id.as_str()),
		);
		assert_eq!(
			items[0].get("server_group_id").and_then(|v| v.as_str()),
			Some(group_id.to_string().as_str()),
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ungrouped_server_event_skips_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		// A server with no group_id should still record issues, but no
		// incident is opened — incidents are group-keyed.
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind) VALUES \
				('{server_id}', 'https://orphan.example.com', 'central');"
		))
		.await
		.expect("seed");

		seed_filed_check(
			&mut conn,
			server_id,
			"x",
			CheckResult::Failed,
			false,
			"no group yet",
		)
		.await;

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert!(items.is_empty(), "ungrouped servers can't have incidents");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn assigning_group_opens_pending_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let server_id = Uuid::new_v4();
		let group_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind) VALUES \
				('{server_id}', 'https://late.example.com', 'central');
			 INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'late group');"
		))
		.await
		.expect("seed");

		// File a check while ungrouped: issue exists, no incident opens.
		seed_filed_check(
			&mut conn,
			server_id,
			"stuck",
			CheckResult::Failed,
			false,
			"waiting to be grouped",
		)
		.await;

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert!(items.is_empty(), "no incident while ungrouped");

		// Assign the server to a group: open issue should now have an incident.
		let resp = private
			.post("/api/servers/update")
			.json(&serde_json::json!({
				"server_id": server_id,
				"data": { "group_id": group_id }
			}))
			.await;
		resp.assert_status_ok();

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "group assignment promoted the open issue");
		assert_eq!(
			items[0].get("server_group_id").and_then(|v| v.as_str()),
			Some(group_id.to_string().as_str()),
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn issue_reopen_keeps_identity_and_joins_new_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let group_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g'); \
			 INSERT INTO servers (id, host, kind, group_id) VALUES \
				('{server_id}', 'https://example.com', 'central', '{group_id}');"
		))
		.await
		.expect("seed");

		// 1. Open with a failure.
		let issue_id_1 = seed_filed_check(
			&mut conn,
			server_id,
			"x",
			CheckResult::Failed,
			false,
			"trouble",
		)
		.await
		.id;

		// 2. Recover — a passing observation closes the issue.
		let issue_id_2 =
			seed_filed_check(&mut conn, server_id, "x", CheckResult::Passed, false, "ok")
				.await
				.id;
		assert_eq!(issue_id_1, issue_id_2, "same identity through inactive");

		// The recovery leaves the incident lingering; let the window
		// elapse so the reopen below is a genuinely new incident rather
		// than a rejoin of the lingering one.
		conn.batch_execute(
			"UPDATE incidents SET closing_at = closing_at - INTERVAL '1 hour' \
			 WHERE closing_at IS NOT NULL",
		)
		.await
		.expect("expire linger");
		database::issues::sweep_lingering_incidents(&mut conn)
			.await
			.expect("linger sweep");

		// 3. Reopen — same identity, severity ≥ error.
		let issue_id_3 =
			seed_filed_check(&mut conn, server_id, "x", CheckResult::Failed, true, "back")
				.await
				.id;
		assert_eq!(issue_id_1, issue_id_3, "reopen keeps identity");

		// Two incidents on the server (first closed, second open).
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({
				"server_id": server_id,
				"include_closed": true,
			}))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 2, "two incidents: one closed, one open");
		// list returns newest first by opened_at desc.
		assert!(items[0].get("closed_at").map_or(false, |v| v.is_null()));
		assert!(items[1].get("closed_at").is_some_and(|v| !v.is_null()));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn low_severity_issue_joins_existing_open_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let group_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g'); \
			 INSERT INTO servers (id, host, kind, group_id) VALUES \
				('{server_id}', 'https://example.com', 'central', '{group_id}');"
		))
		.await
		.expect("seed");

		// 1. Open an incident with a failure.
		seed_filed_check(
			&mut conn,
			server_id,
			"a",
			CheckResult::Failed,
			false,
			"primary trouble",
		)
		.await;

		// 2. A warning would normally not open an incident on its own,
		//    but because one is already open it should join in.
		seed_filed_check(
			&mut conn,
			server_id,
			"b",
			CheckResult::Warning,
			false,
			"ride-along",
		)
		.await;

		// Still one incident, with two contributing issues.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		let incident_id = items[0].get("id").and_then(|v| v.as_str()).unwrap();

		let resp = private
			.post("/api/incidents/get")
			.json(&serde_json::json!({ "incident_id": incident_id }))
			.await;
		let body: serde_json::Value = resp.json();
		let issues = body.get("issues").and_then(|v| v.as_array()).unwrap();
		assert_eq!(issues.len(), 2, "warning piggybacks on the open incident");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn low_severity_alone_does_not_open_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind) VALUES \
				('{server_id}', 'https://example.com', 'central');"
		))
		.await
		.expect("seed");

		// Warning with no open incident: must not create one.
		seed_filed_check(
			&mut conn,
			server_id,
			"b",
			CheckResult::Warning,
			false,
			"minor",
		)
		.await;

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert!(
			items.is_empty(),
			"low-severity alone must not open incident"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn severity_downgrade_keeps_issue_in_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let group_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g'); \
			 INSERT INTO servers (id, host, kind, group_id) VALUES \
				('{server_id}', 'https://example.com', 'central', '{group_id}');"
		))
		.await
		.expect("seed");

		// Open with a failure.
		seed_filed_check(
			&mut conn,
			server_id,
			"x",
			CheckResult::Failed,
			false,
			"trouble",
		)
		.await;

		// Downgrade to warning — still active.
		seed_filed_check(
			&mut conn,
			server_id,
			"x",
			CheckResult::Warning,
			false,
			"less bad now",
		)
		.await;

		// Incident should still be open.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "downgrade should not close the incident");
		assert!(items[0].get("closed_at").map_or(true, |v| v.is_null()));
	})
	.await;
}

async fn open_issue(conn: &mut database::diesel_async::AsyncPgConnection, server_id: Uuid) -> Uuid {
	// Standalone server in its own group — incidents are group-keyed.
	let group_id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g') ON CONFLICT DO NOTHING; \
		 INSERT INTO servers (id, host, kind, group_id) VALUES \
			('{server_id}', 'https://example.com', 'central', '{group_id}') ON CONFLICT DO NOTHING;"
	))
	.await
	.expect("seed");

	seed_filed_check(conn, server_id, "x", CheckResult::Failed, false, "trouble")
		.await
		.id
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_closes_incident_and_records_reason() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let issue_id = open_issue(&mut conn, server_id).await;

		let r = private
			.post("/api/issues/resolve")
			.json(&serde_json::json!({ "issue_id": issue_id, "reason": "fixed" }))
			.await;
		r.assert_status_ok();
		let body: serde_json::Value = r.json();
		assert_eq!(
			body.get("resolved_reason").and_then(|v| v.as_str()),
			Some("fixed")
		);
		// Issue.active stays true — device hasn't said otherwise.
		assert_eq!(body.get("active").and_then(|v| v.as_bool()), Some(true));

		// Incident closed (last contributor left because issue was human-resolved).
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert!(
			items[0].get("closed_at").is_some_and(|v| !v.is_null()),
			"incident should be closed"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unresolve_reopens_incident_if_still_active() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let issue_id = open_issue(&mut conn, server_id).await;

		private
			.post("/api/issues/resolve")
			.json(&serde_json::json!({ "issue_id": issue_id, "reason": "fixed" }))
			.await
			.assert_status_ok();

		private
			.post("/api/issues/unresolve")
			.json(&serde_json::json!({ "issue_id": issue_id }))
			.await
			.assert_status_ok();

		// A new incident should have opened.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(
			items.len(),
			2,
			"one closed (from resolve), one open (from unresolve)"
		);
		// list returns newest first; the open one is items[0].
		assert!(items[0].get("closed_at").map_or(false, |v| v.is_null()));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reopen_via_device_clears_resolved_fields() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, private| {
			let server_id = Uuid::new_v4();
			conn.batch_execute(&format!(
				"INSERT INTO servers (id, host, kind, device_id) VALUES \
					('{server_id}', 'https://example.com', 'central', '{device_id}');"
			))
			.await
			.expect("seed");

			// Device opens the issue by pushing a failing check.
			public
				.post(&format!("/status/{server_id}"))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"health": [ { "check": "x", "result": "failed" } ],
				}))
				.await
				.assert_status_ok();
			let issue = database::issues::Issue::list_by_source_ref(
				&mut conn,
				"alertd",
				"health/x",
				&[server_id],
			)
			.await
			.expect("list issues")
			.into_iter()
			.next()
			.expect("issue opened by the failing check");
			assert!(issue.active);
			let issue_id = issue.id.to_string();

			// Human resolves.
			private
				.post("/api/issues/resolve")
				.json(&serde_json::json!({ "issue_id": issue_id, "reason": "fixed" }))
				.await
				.assert_status_ok();

			// Device pushes again — should clear resolved_* (Sentry-style reopen).
			public
				.post(&format!("/status/{server_id}"))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"health": [ { "check": "x", "result": "failed" } ],
				}))
				.await
				.assert_status_ok();
			let reopened = database::issues::Issue::list_by_source_ref(
				&mut conn,
				"alertd",
				"health/x",
				&[server_id],
			)
			.await
			.expect("list issues")
			.into_iter()
			.next()
			.expect("issue still present after reopen");
			assert!(reopened.active);
			assert!(
				reopened.resolved_at.is_none(),
				"reopen should clear resolved_at"
			);
			assert!(reopened.resolved_by.is_none());
			assert!(reopened.resolved_reason.is_none());
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn snooze_leaves_incident_and_blocks_rejoin() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let issue_id = open_issue(&mut conn, server_id).await;

		// Snooze until far future.
		private
			.post("/api/issues/snooze")
			.json(&serde_json::json!({
				"issue_id": issue_id,
				"until": "2099-01-01T00:00:00Z",
			}))
			.await
			.assert_status_ok();

		// Incident should have closed (issue left).
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert!(
			items[0].get("closed_at").is_some_and(|v| !v.is_null()),
			"incident should be closed"
		);

		// A new failure during snooze should *not* open a new incident.
		seed_filed_check(
			&mut conn,
			server_id,
			"x",
			CheckResult::Failed,
			true,
			"still flapping",
		)
		.await;

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "snooze should suppress new incidents");
		assert!(items[0].get("closed_at").is_some_and(|v| !v.is_null()));

		// Unsnooze → rejoin (active+severity is still met).
		private
			.post("/api/issues/unsnooze")
			.json(&serde_json::json!({ "issue_id": issue_id }))
			.await
			.assert_status_ok();

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(
			items.len(),
			2,
			"unsnooze should reopen incident if still eligible"
		);
		assert!(items[0].get("closed_at").map_or(false, |v| v.is_null()));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unmonitored_server_event_does_not_open_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let server_id = Uuid::new_v4();
		let group_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g');
			 INSERT INTO servers (id, host, kind, group_id, is_monitored) VALUES \
				('{server_id}', 'https://muted.example.com', 'central', '{group_id}', FALSE);"
		))
		.await
		.expect("seed");

		// A failing check normally opens an incident. The server is
		// unmonitored, so the issue is recorded but no incident fires.
		seed_filed_check(
			&mut conn,
			server_id,
			"ignored",
			CheckResult::Failed,
			false,
			"should not open an incident",
		)
		.await;

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert!(items.is_empty(), "unmonitored servers don't open incidents");

		// The issue itself is still there for the record.
		let resp = private
			.post("/api/issues/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let issues: Vec<serde_json::Value> = resp.json();
		assert_eq!(issues.len(), 1, "issue rows are kept even when unmonitored");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn enabling_monitoring_opens_pending_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let server_id = Uuid::new_v4();
		let group_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g');
			 INSERT INTO servers (id, host, kind, group_id, is_monitored) VALUES \
				('{server_id}', 'https://later.example.com', 'central', '{group_id}', FALSE);"
		))
		.await
		.expect("seed");

		// File an issue while unmonitored: no incident.
		seed_filed_check(
			&mut conn,
			server_id,
			"stuck",
			CheckResult::Failed,
			false,
			"waiting to be re-enabled",
		)
		.await;

		// Flip monitoring on: the open issue should be promoted.
		let resp = private
			.post("/api/servers/update")
			.json(&serde_json::json!({
				"server_id": server_id,
				"data": { "is_monitored": true },
			}))
			.await;
		resp.assert_status_ok();

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "monitor-on promoted the open issue");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn disabling_monitoring_removes_open_contribution() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let server_id = Uuid::new_v4();
		open_issue(&mut conn, server_id).await;

		// Sanity: there's an incident before we flip.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		assert_eq!(resp.json::<Vec<serde_json::Value>>().len(), 1);

		// Flip monitoring off: the lone contributor leaves, incident closes.
		let resp = private
			.post("/api/servers/update")
			.json(&serde_json::json!({
				"server_id": server_id,
				"data": { "is_monitored": false },
			}))
			.await;
		resp.assert_status_ok();

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "incident row remains for history");
		assert!(
			items[0].get("closed_at").is_some_and(|v| !v.is_null()),
			"unmonitor closed the incident",
		);

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert!(items.is_empty(), "no open incidents after unmonitor");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn silencing_server_ref_closes_only_matching_open_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let server_id = Uuid::new_v4();
		let _issue_id = open_issue(&mut conn, server_id).await;

		// File a *different* ref on the same server — also an incident-class
		// issue. Two incidents, or one incident with two contributors? Per
		// the existing semantics, the first opens a group incident and the
		// second joins it. We'll see one open incident with two contributors.
		seed_filed_check(
			&mut conn,
			server_id,
			"other",
			CheckResult::Failed,
			false,
			"second contributor",
		)
		.await;
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		assert_eq!(resp.json::<Vec<serde_json::Value>>().len(), 1);

		// Silence the first ref at server scope. The second contributor
		// keeps the incident open.
		let resp = private
			.post("/api/silenced_refs/silence_server")
			.json(&serde_json::json!({
				"server_id": server_id,
				"source": "canopy",
				"ref": "x",
			}))
			.await;
		resp.assert_status_ok();

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "still open via the unsilenced contributor");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unsilencing_server_ref_rejoins_open_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let server_id = Uuid::new_v4();
		let _issue_id = open_issue(&mut conn, server_id).await;

		// Silence then unsilence: the (re-)evaluation should leave the issue
		// in the same state we started in.
		let resp = private
			.post("/api/silenced_refs/silence_server")
			.json(&serde_json::json!({
				"server_id": server_id,
				"source": "canopy",
				"ref": "x",
			}))
			.await;
		resp.assert_status_ok();
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "incident row exists for history");
		assert!(
			items[0].get("closed_at").is_some_and(|v| !v.is_null()),
			"silenced lone contributor closes the incident",
		);

		let resp = private
			.post("/api/silenced_refs/unsilence_server")
			.json(&serde_json::json!({
				"server_id": server_id,
				"source": "canopy",
				"ref": "x",
			}))
			.await;
		resp.assert_status_ok();

		// Unsilence reopens via a fresh incident (the old one stays closed).
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "fresh incident after unsilence");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn group_silence_blocks_events_from_all_members() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let group_id = Uuid::new_v4();
		let server_a = Uuid::new_v4();
		let server_b = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g');
			 INSERT INTO servers (id, host, kind, group_id) VALUES \
				('{server_a}', 'https://a.example.com', 'central', '{group_id}'),
				('{server_b}', 'https://b.example.com', 'central', '{group_id}');"
		))
		.await
		.expect("seed");

		// Silence the ref at group scope.
		let resp = private
			.post("/api/silenced_refs/silence_group")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"source": "canopy",
				"ref": "noisy",
			}))
			.await;
		resp.assert_status_ok();

		// Either member firing the silenced ref doesn't open an incident.
		for sid in [server_a, server_b] {
			seed_filed_check(
				&mut conn,
				sid,
				"noisy",
				CheckResult::Failed,
				false,
				"should not fire",
			)
			.await;
		}

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_a }))
			.await;
		assert!(resp.json::<Vec<serde_json::Value>>().is_empty());

		// A different ref still opens an incident — silence is ref-specific.
		seed_filed_check(
			&mut conn,
			server_a,
			"other",
			CheckResult::Failed,
			false,
			"should still fire",
		)
		.await;
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_a }))
			.await;
		assert_eq!(resp.json::<Vec<serde_json::Value>>().len(), 1);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn list_silenced_refs_for_server_and_group() {
	commons_tests::server::run(async |mut conn, _public, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.expect("seed admin");

		let group_id = Uuid::new_v4();
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'g');
			 INSERT INTO servers (id, host, kind, group_id) VALUES \
				('{server_id}', 'https://l.example.com', 'central', '{group_id}');"
		))
		.await
		.expect("seed");

		private
			.post("/api/silenced_refs/silence_server")
			.json(&serde_json::json!({
				"server_id": server_id,
				"source": "canopy",
				"ref": "srv-ref",
			}))
			.await
			.assert_status_ok();
		private
			.post("/api/silenced_refs/silence_group")
			.json(&serde_json::json!({
				"server_group_id": group_id,
				"source": "canopy",
				"ref": "grp-ref",
			}))
			.await
			.assert_status_ok();

		let resp = private
			.post("/api/silenced_refs/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert_eq!(
			items[0].get("ref").and_then(|v| v.as_str()),
			Some("srv-ref")
		);

		let resp = private
			.post("/api/silenced_refs/list_for_group")
			.json(&serde_json::json!({ "server_group_id": group_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert_eq!(
			items[0].get("ref").and_then(|v| v.as_str()),
			Some("grp-ref")
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn incident_resolve_metadata() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		open_issue(&mut conn, server_id).await;

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		let incident_id = items[0].get("id").unwrap().as_str().unwrap().to_string();

		// Incident resolve cascades to the issues inside, which auto-closes
		// the incident — operators shouldn't see a 'resolved but still open'
		// state.
		let resolved = private
			.post("/api/incidents/resolve")
			.json(&serde_json::json!({ "incident_id": incident_id, "reason": "expected" }))
			.await;
		resolved.assert_status_ok();
		let body: serde_json::Value = resolved.json();
		assert_eq!(
			body.get("resolved_reason").and_then(|v| v.as_str()),
			Some("expected")
		);
		assert!(
			body.get("closed_at").is_some_and(|v| !v.is_null()),
			"resolve closes the incident"
		);
	})
	.await;
}
