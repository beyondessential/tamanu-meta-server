use commons_tests::diesel_async::SimpleAsyncConnection;
use uuid::Uuid;

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
			 INSERT INTO issues (server_id, device_id, source, \"ref\", severity, message, active, first_seen, last_seen) VALUES \
				('{server_id}', '{device_id}', 'src', 'a', 'error',    'newest', true,  '2026-05-03T10:00:00Z', '2026-05-03T10:00:00Z'),
				('{server_id}', '{device_id}', 'src', 'b', 'warning',  'older',  true,  '2026-05-01T10:00:00Z', '2026-05-01T10:00:00Z'),
				('{server_id}', '{device_id}', 'src', 'c', 'info',     'gone',   false, '2026-05-02T10:00:00Z', '2026-05-02T10:00:00Z');"
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
async fn manual_event_submit_creates_issue_without_device() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind) VALUES \
				('{server_id}', 'https://example.com', 'central');"
		))
		.await
		.expect("seed");

		let resp = private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": server_id,
				"ref": "operator-note-1",
				"message": "manually opened",
			}))
			.await;
		resp.assert_status_ok();
		let body: serde_json::Value = resp.json();
		assert_eq!(body.get("source").and_then(|v| v.as_str()), Some("manual"));
		assert!(body.get("device_id").map_or(true, |v| v.is_null()));
		assert_eq!(body.get("severity").and_then(|v| v.as_str()), Some("error"));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn incident_groups_at_root_server() {
	commons_tests::server::run(async |mut conn, _public, private| {
		// root --child (with device)
		let device_id = Uuid::new_v4();
		let root_id = Uuid::new_v4();
		let child_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO devices (id, role) VALUES ('{device_id}', 'server');
			 INSERT INTO device_keys (device_id, key_data, name, is_active) VALUES \
				('{device_id}', '\\x6b6579'::bytea, 'k', true);
			 INSERT INTO servers (id, host, kind) VALUES \
				('{root_id}', 'https://root.example.com', 'central');
			 INSERT INTO servers (id, host, kind, device_id, parent_server_id) VALUES \
				('{child_id}', 'https://child.example.com', 'facility', '{device_id}', '{root_id}');"
		))
		.await
		.expect("seed");

		// Submit a manual event on the child server with severity=error → opens incident at root.
		let resp = private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": child_id,
				"ref": "x",
				"severity": "error",
				"message": "trouble in child",
			}))
			.await;
		resp.assert_status_ok();

		// Root should have one open incident.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": root_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "incident lives on the root server");
		assert!(items[0].get("closed_at").map_or(true, |v| v.is_null()));

		// Child has no incident of its own.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": child_id }))
			.await;
		resp.assert_status_ok();
		let items: Vec<serde_json::Value> = resp.json();
		assert!(items.is_empty(), "child server doesn't host the incident");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn issue_reopen_keeps_identity_and_joins_new_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind) VALUES \
				('{server_id}', 'https://example.com', 'central');"
		))
		.await
		.expect("seed");

		// 1. Open with error.
		let r1 = private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": server_id,
				"ref": "x",
				"severity": "error",
				"message": "trouble",
			}))
			.await;
		r1.assert_status_ok();
		let issue_id_1 = r1.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();

		// 2. Resolve.
		let r2 = private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": server_id,
				"ref": "x",
				"severity": "error",
				"active": false,
				"message": "ok",
			}))
			.await;
		r2.assert_status_ok();
		let issue_id_2 = r2.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();
		assert_eq!(issue_id_1, issue_id_2, "same identity through inactive");

		// 3. Reopen — same identity, severity ≥ error.
		let r3 = private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": server_id,
				"ref": "x",
				"severity": "critical",
				"message": "back",
			}))
			.await;
		r3.assert_status_ok();
		let issue_id_3 = r3.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();
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
async fn severity_downgrade_keeps_issue_in_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind) VALUES \
				('{server_id}', 'https://example.com', 'central');"
		))
		.await
		.expect("seed");

		// Open at error.
		private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": server_id,
				"ref": "x",
				"severity": "error",
				"message": "trouble",
			}))
			.await
			.assert_status_ok();

		// Downgrade to warning — still active.
		private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": server_id,
				"ref": "x",
				"severity": "warning",
				"message": "less bad now",
			}))
			.await
			.assert_status_ok();

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

#[tokio::test(flavor = "multi_thread")]
async fn list_events_returns_event_log() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO servers (id, host, kind) VALUES \
				('{server_id}', 'https://example.com', 'central');"
		))
		.await
		.expect("seed");

		// Three distinct events.
		for (sev, msg) in [
			("error", "a"),
			("error", "b"),
			("warning", "b"),
		] {
			private
				.post("/api/issues/submit_manual_event")
				.json(&serde_json::json!({
					"serverId": server_id,
					"ref": "x",
					"severity": sev,
					"message": msg,
				}))
				.await
				.assert_status_ok();
		}

		// Find the issue.
		let issues = private
			.post("/api/issues/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		issues.assert_status_ok();
		let items: Vec<serde_json::Value> = issues.json();
		assert_eq!(items.len(), 1);
		let issue_id = items[0].get("id").unwrap().as_str().unwrap();

		let events = private
			.post("/api/issues/list_events")
			.json(&serde_json::json!({ "issue_id": issue_id }))
			.await;
		events.assert_status_ok();
		let evs: Vec<serde_json::Value> = events.json();
		assert_eq!(evs.len(), 3, "each distinct push is its own event row");
	})
	.await;
}

async fn open_issue(
	conn: &mut database::diesel_async::AsyncPgConnection,
	private: &commons_tests::axum_test::TestServer,
	server_id: Uuid,
) -> Uuid {
	// Standalone server, no parent — the incident grouping lives on this server.
	conn.batch_execute(&format!(
		"INSERT INTO servers (id, host, kind) VALUES \
			('{server_id}', 'https://example.com', 'central') ON CONFLICT DO NOTHING;"
	))
	.await
	.expect("seed");

	let r = private
		.post("/api/issues/submit_manual_event")
		.json(&serde_json::json!({
			"serverId": server_id,
			"ref": "x",
			"severity": "error",
			"message": "trouble",
		}))
		.await;
	r.assert_status_ok();
	Uuid::parse_str(r.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn ack_does_not_close_incident() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let issue_id = open_issue(&mut conn, &private, server_id).await;

		let r = private
			.post("/api/issues/ack")
			.json(&serde_json::json!({ "issue_id": issue_id }))
			.await;
		r.assert_status_ok();
		let body: serde_json::Value = r.json();
		assert!(body.get("acknowledged_at").is_some_and(|v| !v.is_null()));
		assert_eq!(
			body.get("acknowledged_by").and_then(|v| v.as_str()),
			Some("admin@localhost"),
		);

		// Incident should still be open.
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1, "ack must not close the incident");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_closes_incident_and_records_reason() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let issue_id = open_issue(&mut conn, &private, server_id).await;

		let r = private
			.post("/api/issues/resolve")
			.json(&serde_json::json!({ "issue_id": issue_id, "reason": "fixed" }))
			.await;
		r.assert_status_ok();
		let body: serde_json::Value = r.json();
		assert_eq!(body.get("resolved_reason").and_then(|v| v.as_str()), Some("fixed"));
		// Issue.active stays true — device hasn't said otherwise.
		assert_eq!(body.get("active").and_then(|v| v.as_bool()), Some(true));

		// Incident closed (last contributor left because issue was human-resolved).
		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id, "include_closed": true }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		assert_eq!(items.len(), 1);
		assert!(items[0].get("closed_at").is_some_and(|v| !v.is_null()), "incident should be closed");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unresolve_reopens_incident_if_still_active() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let issue_id = open_issue(&mut conn, &private, server_id).await;

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
		assert_eq!(items.len(), 2, "one closed (from resolve), one open (from unresolve)");
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

			// Device opens the issue.
			let opened = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"severity": "error",
					"message": "trouble",
				}))
				.await;
			opened.assert_status_ok();
			let issue_id = opened.json::<serde_json::Value>().get("id").unwrap().as_str().unwrap().to_string();

			// Human resolves.
			private
				.post("/api/issues/resolve")
				.json(&serde_json::json!({ "issue_id": issue_id, "reason": "fixed" }))
				.await
				.assert_status_ok();

			// Device pushes again — should clear resolved_* (Sentry-style reopen).
			let reopened = public
				.post("/events")
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({
					"source": "watchdog",
					"ref": "x",
					"severity": "error",
					"message": "back again",
				}))
				.await;
			reopened.assert_status_ok();
			let body: serde_json::Value = reopened.json();
			assert!(body.get("resolved_at").map_or(true, |v| v.is_null()), "reopen should clear resolved_at");
			assert!(body.get("resolved_by").map_or(true, |v| v.is_null()));
			assert!(body.get("resolved_reason").map_or(true, |v| v.is_null()));
		},
	)
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn snooze_leaves_incident_and_blocks_rejoin() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		let issue_id = open_issue(&mut conn, &private, server_id).await;

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
		assert!(items[0].get("closed_at").is_some_and(|v| !v.is_null()), "incident should be closed");

		// A new error event during snooze should *not* open a new incident.
		private
			.post("/api/issues/submit_manual_event")
			.json(&serde_json::json!({
				"serverId": server_id,
				"ref": "x",
				"severity": "critical",
				"message": "still flapping",
			}))
			.await
			.assert_status_ok();

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
		assert_eq!(items.len(), 2, "unsnooze should reopen incident if still eligible");
		assert!(items[0].get("closed_at").map_or(false, |v| v.is_null()));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn incident_ack_resolve_metadata() {
	commons_tests::server::run(async |mut conn, _public, private| {
		let server_id = Uuid::new_v4();
		open_issue(&mut conn, &private, server_id).await;

		let resp = private
			.post("/api/incidents/list_for_server")
			.json(&serde_json::json!({ "server_id": server_id }))
			.await;
		let items: Vec<serde_json::Value> = resp.json();
		let incident_id = items[0].get("id").unwrap().as_str().unwrap().to_string();

		let acked = private
			.post("/api/incidents/ack")
			.json(&serde_json::json!({ "incident_id": incident_id }))
			.await;
		acked.assert_status_ok();
		let body: serde_json::Value = acked.json();
		assert!(body.get("acknowledged_at").is_some_and(|v| !v.is_null()));

		// Incident resolve is metadata only — doesn't force closed_at.
		let resolved = private
			.post("/api/incidents/resolve")
			.json(&serde_json::json!({ "incident_id": incident_id, "reason": "expected" }))
			.await;
		resolved.assert_status_ok();
		let body: serde_json::Value = resolved.json();
		assert_eq!(body.get("resolved_reason").and_then(|v| v.as_str()), Some("expected"));
		assert!(body.get("closed_at").map_or(true, |v| v.is_null()), "resolve doesn't force-close");
	})
	.await;
}
