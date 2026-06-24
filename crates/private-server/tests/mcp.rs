//! Tests for the read-only MCP query interface mounted at `/api/mcp`.
//!
//! The debug build bypasses Tailscale auth, so no headers are needed. These
//! drive the real protocol path (initialize → tools/call over Streamable HTTP)
//! against seeded data and assert the structured results, so they cover both
//! the wiring and each tool's data shaping.

use commons_tests::diesel_async::SimpleAsyncConnection;

/// The Streamable HTTP transport requires the client to accept both JSON and
/// SSE on the POST leg.
const ACCEPT: &str = "application/json, text/event-stream";

/// Extract the JSON-RPC envelope from a Streamable HTTP response body, which is
/// either a bare JSON object or an SSE stream whose `data:` line carries it.
fn parse_envelope(body: &str) -> serde_json::Value {
	let trimmed = body.trim_start();
	if trimmed.starts_with('{') {
		return serde_json::from_str(trimmed).expect("json body");
	}
	let data = body
		.lines()
		.filter_map(|l| l.strip_prefix("data:"))
		.last()
		.unwrap_or_else(|| panic!("no SSE data line in: {body}"));
	serde_json::from_str(data.trim()).expect("json in SSE data line")
}

/// Initialize a session and return its id. Expanded inline so the test never
/// has to name `axum_test::TestServer`.
macro_rules! init_session {
	($private:expr) => {{
		let init = $private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "id": 1, "method": "initialize",
				"params": {
					"protocolVersion": "2025-06-18",
					"capabilities": {},
					"clientInfo": { "name": "canopy-tests", "version": "0" }
				}
			}))
			.await;
		assert_eq!(init.status_code().as_u16(), 200, "initialize should 200");
		let session = init
			.headers()
			.get("mcp-session-id")
			.expect("session id")
			.to_str()
			.unwrap()
			.to_owned();
		$private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-session-id", &session)
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "method": "notifications/initialized"
			}))
			.await;
		session
	}};
}

/// Call a tool and return its `structuredContent`. Asserts the call succeeded
/// (no JSON-RPC error and `isError` is not true).
macro_rules! call_tool {
	($private:expr, $session:expr, $name:expr, $args:expr) => {{
		let resp = $private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-session-id", &$session)
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "id": 2, "method": "tools/call",
				"params": { "name": $name, "arguments": $args }
			}))
			.await;
		assert_eq!(resp.status_code().as_u16(), 200, "{} should 200", $name);
		let env = parse_envelope(&resp.text());
		assert!(env.get("error").is_none(), "{} rpc error: {env}", $name);
		let result = env.get("result").expect("result").clone();
		assert_ne!(
			result.get("isError").and_then(|v| v.as_bool()),
			Some(true),
			"{} returned a tool error: {result}",
			$name
		);
		result
			.get("structuredContent")
			.cloned()
			.unwrap_or(serde_json::Value::Null)
	}};
}

/// Seed a group, two servers (one grouped + monitored with a fresh healthy
/// status, one ungrouped), and a backup config + capability + schedule.
const GROUP: &str = "11111111-1111-1111-1111-111111111111";
const SRV_GROUPED: &str = "22222222-2222-2222-2222-222222222222";
const SRV_UNGROUPED: &str = "33333333-3333-3333-3333-333333333333";

async fn seed(conn: &mut impl SimpleAsyncConnection) {
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{GROUP}', 'Prod Group'); \
		 INSERT INTO servers (id, host, name, kind, rank, group_id, is_monitored) VALUES \
			('{SRV_GROUPED}', 'https://prod-central', 'Prod Central', 'central', 'production', '{GROUP}', true); \
		 INSERT INTO servers (id, host, name, kind) VALUES \
			('{SRV_UNGROUPED}', 'https://lonely', 'Lonely Facility', 'facility'); \
		 INSERT INTO statuses (server_id, version, healthy, health, extra, created_at) VALUES \
			('{SRV_GROUPED}', '2.34.1', true, '[]'::jsonb, \
			 '{{\"pgVersion\": \"PostgreSQL 14.2 on x86_64-pc-linux-gnu\"}}'::jsonb, NOW() - interval '1 minute');"
	))
	.await
	.expect("seed");
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_and_list_tools() {
	commons_tests::server::run(async |_conn, _public, private| {
		let session = init_session!(private);
		let list = private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-session-id", &session)
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
			}))
			.await;
		assert_eq!(list.status_code().as_u16(), 200);
		let body = list.text();
		for tool in [
			"find_servers",
			"get_server",
			"find_groups",
			"get_group",
			"list_versions",
			"get_version",
			"fleet_summary",
			"find_backup_problems",
		] {
			assert!(body.contains(tool), "tools/list missing {tool}: {body}");
		}
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn find_servers_filters_and_decorates() {
	commons_tests::server::run(async |mut conn, _public, private| {
		seed(&mut conn).await;
		let session = init_session!(private);

		// Unfiltered: both seeded servers.
		let all = call_tool!(private, session, "find_servers", serde_json::json!({}));
		assert!(all["total_matched"].as_u64().unwrap() >= 2);

		// Filter by kind=facility → only the ungrouped one.
		let facility = call_tool!(
			private,
			session,
			"find_servers",
			serde_json::json!({ "kind": "facility" })
		);
		let servers = facility["servers"].as_array().unwrap();
		assert_eq!(servers.len(), 1);
		assert_eq!(servers[0]["id"], SRV_UNGROUPED);
		assert_eq!(servers[0]["health"], "healthy");

		// Query matches by name.
		let q = call_tool!(
			private,
			session,
			"find_servers",
			serde_json::json!({ "query": "central" })
		);
		let names: Vec<&str> = q["servers"]
			.as_array()
			.unwrap()
			.iter()
			.map(|s| s["name"].as_str().unwrap())
			.collect();
		assert_eq!(names, vec!["Prod Central"]);

		// Bad enum → invalid params (protocol error).
		let bad = private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-session-id", &session)
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "id": 9, "method": "tools/call",
				"params": { "name": "find_servers", "arguments": { "kind": "nonsense" } }
			}))
			.await;
		let env = parse_envelope(&bad.text());
		assert!(
			env.get("error").is_some() || env["result"]["isError"] == serde_json::json!(true),
			"bad kind should error: {env}"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_server_detail_and_not_found() {
	commons_tests::server::run(async |mut conn, _public, private| {
		seed(&mut conn).await;
		let session = init_session!(private);

		let detail = call_tool!(
			private,
			session,
			"get_server",
			serde_json::json!({ "server_id": SRV_GROUPED })
		);
		assert_eq!(detail["name"], "Prod Central");
		assert_eq!(detail["group_name"], "Prod Group");
		assert_eq!(detail["latest_status"]["version"], "2.34.1");
		assert_eq!(detail["latest_status"]["platform"], "Linux");

		// Unknown id → tool error (isError), not a protocol error.
		let missing = private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-session-id", &session)
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "id": 3, "method": "tools/call",
				"params": {
					"name": "get_server",
					"arguments": { "server_id": "44444444-4444-4444-4444-444444444444" }
				}
			}))
			.await;
		let env = parse_envelope(&missing.text());
		assert_eq!(env["result"]["isError"], serde_json::json!(true));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn group_listing_and_detail() {
	commons_tests::server::run(async |mut conn, _public, private| {
		seed(&mut conn).await;
		let session = init_session!(private);

		let groups = call_tool!(private, session, "find_groups", serde_json::json!({}));
		let g = groups["groups"]
			.as_array()
			.unwrap()
			.iter()
			.find(|g| g["id"] == GROUP)
			.expect("seeded group present");
		assert_eq!(g["name"], "Prod Group");
		assert_eq!(g["member_count"], 1);
		assert_eq!(g["highest_rank"], "production");

		let detail = call_tool!(
			private,
			session,
			"get_group",
			serde_json::json!({ "group_id": GROUP })
		);
		assert_eq!(detail["name"], "Prod Group");
		let members = detail["members"].as_array().unwrap();
		assert_eq!(members.len(), 1);
		assert_eq!(members[0]["id"], SRV_GROUPED);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn fleet_summary_rolls_up() {
	commons_tests::server::run(async |mut conn, _public, private| {
		seed(&mut conn).await;
		let session = init_session!(private);

		let s = call_tool!(private, session, "fleet_summary", serde_json::json!({}));
		assert!(s["total_servers"].as_u64().unwrap() >= 2);
		assert_eq!(s["counts"]["by_kind"]["facility"], 1);
		assert_eq!(s["version_distribution"]["2.34.1"], 1);
		assert_eq!(s["health"]["healthy"], 1);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_problems_scan_runs() {
	commons_tests::server::run(async |mut conn, _public, private| {
		seed(&mut conn).await;
		let session = init_session!(private);
		// No ready backup config seeded, so the scan returns an empty, well-formed set.
		let p = call_tool!(
			private,
			session,
			"find_backup_problems",
			serde_json::json!({})
		);
		assert!(p["count"].is_number());
		assert!(p["problems"].is_array());
	})
	.await
}
