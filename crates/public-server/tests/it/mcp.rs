//! The internet-facing MCP mount at `/mcp` is gated by bearer tokens: no
//! token, a malformed header, an unknown token, and a revoked token all get
//! the same opaque 401 with a `WWW-Authenticate: Bearer` challenge; a minted
//! token calls tools; hammering failures trips the per-IP rate limit.

use database::manual_incidents::ManualIncident;
use database::mcp_tokens::McpToken;

/// Clients must accept both JSON and SSE on the POST leg.
const ACCEPT: &str = "application/json, text/event-stream";
/// Required on every non-initialize request in stateless mode.
const PROTO: &str = "2025-06-18";

fn fleet_summary_call() -> serde_json::Value {
	tool_call("fleet_summary", serde_json::json!({}))
}

fn tool_call(name: &str, args: serde_json::Value) -> serde_json::Value {
	serde_json::json!({
		"jsonrpc": "2.0", "id": 1, "method": "tools/call",
		"params": { "name": name, "arguments": args }
	})
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_without_a_usable_token() {
	commons_tests::server::run(async |mut conn, public, _private| {
		// No Authorization header at all.
		let bare = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.json(&fleet_summary_call())
			.await;
		assert_eq!(bare.status_code().as_u16(), 401);
		assert_eq!(
			bare.headers()
				.get("www-authenticate")
				.and_then(|v| v.to_str().ok()),
			Some("Bearer"),
		);

		// Wrong scheme.
		let basic = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.add_header("authorization", "Basic dXNlcjpwYXNz")
			.json(&fleet_summary_call())
			.await;
		assert_eq!(basic.status_code().as_u16(), 401);

		// Well-formed but unknown token.
		let unknown = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.add_header("authorization", "Bearer canopy_mcp_definitely-not-minted")
			.json(&fleet_summary_call())
			.await;
		assert_eq!(unknown.status_code().as_u16(), 401);

		// Revoked token.
		let (token, plaintext) = McpToken::mint(&mut conn, "revoked", "test@example.com", false)
			.await
			.expect("mint");
		McpToken::revoke(&mut conn, token.id).await.expect("revoke");
		let revoked = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.add_header("authorization", format!("Bearer {plaintext}"))
			.json(&fleet_summary_call())
			.await;
		assert_eq!(revoked.status_code().as_u16(), 401);

		// The refusals are uniform: same status, same problem type.
		assert_eq!(unknown.text(), revoked.text());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn minted_token_calls_tools_and_records_use() {
	commons_tests::server::run(async |mut conn, public, _private| {
		let (token, plaintext) = McpToken::mint(&mut conn, "claude", "test@example.com", false)
			.await
			.expect("mint");
		assert!(token.last_used_at.is_none());

		let resp = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.add_header("authorization", format!("Bearer {plaintext}"))
			.json(&fleet_summary_call())
			.await;
		assert_eq!(resp.status_code().as_u16(), 200, "body: {}", resp.text());
		let env: serde_json::Value = serde_json::from_str(&resp.text()).expect("json body");
		assert!(env.get("error").is_none(), "rpc error: {env}");
		let result = &env["result"];
		assert_ne!(result.get("isError").and_then(|v| v.as_bool()), Some(true));
		assert!(result["structuredContent"]["total_servers"].is_u64());

		let listed = McpToken::list(&mut conn).await.expect("list");
		assert!(
			listed
				.iter()
				.any(|t| t.id == token.id && t.last_used_at.is_some()),
			"successful call must stamp last_used_at"
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn read_only_token_reads_but_cannot_write_manual_incidents() {
	commons_tests::server::run(async |mut conn, public, _private| {
		let (token, plaintext) = McpToken::mint(&mut conn, "reader", "test@example.com", false)
			.await
			.expect("mint");
		assert!(!token.write_access);
		let auth = format!("Bearer {plaintext}");

		// Reads work: an empty, well-formed list.
		let resp = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.add_header("authorization", &auth)
			.json(&tool_call("find_manual_incidents", serde_json::json!({})))
			.await;
		assert_eq!(resp.status_code().as_u16(), 200, "body: {}", resp.text());
		let env: serde_json::Value = serde_json::from_str(&resp.text()).expect("json body");
		assert!(env.get("error").is_none(), "rpc error: {env}");
		assert_eq!(env["result"]["structuredContent"]["count"], 0);

		// A write attempt is refused with a message naming the fix.
		let resp = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.add_header("authorization", &auth)
			.json(&tool_call(
				"record_manual_incident",
				serde_json::json!({
					"title": "Attempted by a reader",
					"started_at": "2026-07-01T10:00:00Z",
					// Any well-formed group id: the write gate refuses before
					// the group is ever looked up.
					"group_id": "99999999-9999-9999-9999-999999999999",
				}),
			))
			.await;
		assert_eq!(resp.status_code().as_u16(), 200, "body: {}", resp.text());
		let env: serde_json::Value = serde_json::from_str(&resp.text()).expect("json body");
		// The refusal may surface as a JSON-RPC error or a tool error; either
		// way its message must point at the missing write access.
		let message = if let Some(error) = env.get("error") {
			error["message"].as_str().unwrap_or_default().to_string()
		} else {
			assert_eq!(
				env["result"]["isError"],
				serde_json::json!(true),
				"read-only write attempt must fail: {env}"
			);
			env["result"]["content"].to_string()
		};
		assert!(
			message.contains("read-only") || message.contains("write access"),
			"refusal should mention read-only/write access: {message}"
		);

		// And nothing was written.
		let rows = ManualIncident::list(&mut conn, None, false, 10)
			.await
			.expect("list");
		assert!(rows.is_empty(), "read-only caller must not write: {rows:?}");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn write_token_records_manual_incident_attributed_to_token_name() {
	commons_tests::server::run(async |mut conn, public, _private| {
		use commons_tests::diesel_async::SimpleAsyncConnection as _;
		let group_id = uuid::Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'Token Group')"
		))
		.await
		.expect("seed group");
		let (token, plaintext) = McpToken::mint(&mut conn, "scribe", "test@example.com", true)
			.await
			.expect("mint");
		assert!(token.write_access);

		let resp = public
			.post("/mcp")
			.add_header("accept", ACCEPT)
			.add_header("mcp-protocol-version", PROTO)
			.add_header("authorization", format!("Bearer {plaintext}"))
			.json(&tool_call(
				"record_manual_incident",
				serde_json::json!({
					"title": "Fibre cut in Suva",
					"description": "ISP outage.",
					"started_at": "2026-07-01T10:00:00Z",
					"ended_at": "2026-07-01T12:30:00Z",
					"group_id": group_id,
				}),
			))
			.await;
		assert_eq!(resp.status_code().as_u16(), 200, "body: {}", resp.text());
		let env: serde_json::Value = serde_json::from_str(&resp.text()).expect("json body");
		assert!(env.get("error").is_none(), "rpc error: {env}");
		let result = &env["result"];
		assert_ne!(
			result.get("isError").and_then(|v| v.as_bool()),
			Some(true),
			"tool error: {result}"
		);
		let out = &result["structuredContent"];
		assert_eq!(out["title"], "Fibre cut in Suva");
		assert_eq!(out["created_by"], "scribe");
		assert_eq!(out["group_id"], group_id.to_string());
		assert_eq!(out["group_name"], "Token Group");

		// The row exists and is attributed to the token's name.
		let rows = ManualIncident::list(&mut conn, None, false, 10)
			.await
			.expect("list");
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].title, "Fibre cut in Suva");
		assert_eq!(rows[0].created_by, "scribe");
		assert!(rows[0].ended_at.is_some());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_attempts_rate_limit_per_ip() {
	commons_tests::server::run(async |_conn, public, _private| {
		// The failed-auth budget is 30/minute per IP; the 401s must flip to
		// 429 before 40 attempts.
		let mut saw_429 = false;
		for _ in 0..40 {
			let resp = public
				.post("/mcp")
				.add_header("accept", ACCEPT)
				.add_header("mcp-protocol-version", PROTO)
				.add_header("authorization", "Bearer canopy_mcp_guess")
				.json(&fleet_summary_call())
				.await;
			match resp.status_code().as_u16() {
				401 => continue,
				429 => {
					saw_429 = true;
					break;
				}
				other => panic!("unexpected status {other}"),
			}
		}
		assert!(saw_429, "failed attempts never rate limited");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn oauth_discovery_is_404() {
	// MCP clients probe for OAuth authorization-server metadata; the public
	// mount must 404 those so clients fall back to plain bearer auth.
	commons_tests::server::run(async |_conn, public, _private| {
		for path in [
			"/.well-known/oauth-authorization-server",
			"/.well-known/oauth-protected-resource",
		] {
			let resp = public.get(path).await;
			assert_eq!(resp.status_code().as_u16(), 404, "{path}");
		}
	})
	.await
}
