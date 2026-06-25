//! Tests for the read-only MCP query interface mounted at `/api/mcp`.
//!
//! The endpoint runs in stateless mode (no server-side session), so each POST
//! is self-contained: a `tools/call` needs no prior `initialize`, just an
//! `MCP-Protocol-Version` header. Responses are plain `application/json`. The
//! debug build bypasses Tailscale auth, so no identity headers are needed.

use commons_tests::diesel_async::SimpleAsyncConnection;

/// Clients must accept both JSON and SSE on the POST leg.
const ACCEPT: &str = "application/json, text/event-stream";
/// Required on every non-initialize request in stateless mode.
const PROTO: &str = "2025-06-18";

/// Extract the JSON-RPC envelope from a response body (plain JSON in
/// json-response mode, or an SSE `data:` line otherwise).
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

/// Call a tool and return its `structuredContent`. Asserts the call succeeded
/// (no JSON-RPC error and `isError` is not true).
macro_rules! call_tool {
	($private:expr, $name:expr, $args:expr) => {{
		let resp = $private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
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
/// status, one ungrouped), and a status carrying a version + platform.
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
		// initialize works (and returns tool capability), but is not required
		// before other calls in stateless mode.
		let init = private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "id": 1, "method": "initialize",
				"params": {
					"protocolVersion": PROTO,
					"capabilities": {},
					"clientInfo": { "name": "canopy-tests", "version": "0" }
				}
			}))
			.await;
		assert_eq!(init.status_code().as_u16(), 200, "initialize should 200");

		let list = private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
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
async fn tools_call_needs_no_session() {
	// Regression: the endpoint is stateless, so a `tools/call` must succeed on
	// its own — no `initialize` handshake and no session id. (The default
	// stateful mode 404s any request routed to a replica that didn't handle
	// `initialize`, which is what broke real clients behind the load balancer.)
	commons_tests::server::run(async |mut conn, _public, private| {
		seed(&mut conn).await;
		let summary = call_tool!(private, "fleet_summary", serde_json::json!({}));
		assert!(summary["total_servers"].as_u64().unwrap() >= 2);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn oauth_discovery_is_404_not_spa() {
	// MCP clients probe `/.well-known/oauth-*` for OAuth metadata. The SPA
	// fallback used to answer 200 text/html, which clients fail to parse as JSON
	// and report as "needs authentication". A 404 tells them there's no OAuth, so
	// they connect with the ambient (Tailscale) identity instead.
	commons_tests::server::run(async |_conn, _public, private| {
		for path in [
			"/.well-known/oauth-protected-resource",
			"/.well-known/oauth-protected-resource/api/mcp",
			"/.well-known/oauth-authorization-server",
		] {
			let resp = private.get(path).await;
			assert_eq!(
				resp.status_code().as_u16(),
				404,
				"{path} should be 404, not the SPA"
			);
		}
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn accepts_non_loopback_host() {
	// Regression: rmcp's DNS-rebinding guard defaults to a loopback-only Host
	// allowlist, which 403s the real tailnet deployment host. The endpoint is
	// gated by the ingress + tagged-device guard + tailnet-user check instead,
	// so a non-loopback Host must be accepted.
	commons_tests::server::run(async |_conn, _public, private| {
		let init = private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("host", "canopy.example.ts.net")
			.json(&serde_json::json!({
				"jsonrpc": "2.0", "id": 1, "method": "initialize",
				"params": {
					"protocolVersion": PROTO,
					"capabilities": {},
					"clientInfo": { "name": "canopy-tests", "version": "0" }
				}
			}))
			.await;
		assert_eq!(
			init.status_code().as_u16(),
			200,
			"non-loopback Host should be accepted, got: {}",
			init.text()
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn find_servers_filters_and_decorates() {
	commons_tests::server::run(async |mut conn, _public, private| {
		seed(&mut conn).await;

		// Unfiltered: both seeded servers.
		let all = call_tool!(private, "find_servers", serde_json::json!({}));
		assert!(all["total_matched"].as_u64().unwrap() >= 2);

		// Filter by kind=facility → only the ungrouped one.
		let facility = call_tool!(
			private,
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

		// Bad enum → error (protocol or tool-level).
		let bad = private
			.post("/api/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
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

		let detail = call_tool!(
			private,
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
			.add_header("mcp-protocol-version", PROTO)
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

		let groups = call_tool!(private, "find_groups", serde_json::json!({}));
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

		let s = call_tool!(private, "fleet_summary", serde_json::json!({}));
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
		// No ready backup config seeded, so the scan returns an empty, well-formed set.
		let p = call_tool!(private, "find_backup_problems", serde_json::json!({}));
		assert!(p["count"].is_number());
		assert!(p["problems"].is_array());
	})
	.await
}
