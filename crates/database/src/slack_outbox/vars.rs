//! Flat-variable payloads for Slack Workflow Builder webhook triggers.
//!
//! Workflow Builder webhooks don't accept Block Kit directly. Each workflow
//! declares a named set of text variables in its trigger; the actual message
//! is composed inside Slack's workflow editor using those variables. So we
//! POST a flat JSON object keyed by the declared variable names — no
//! `blocks`, no `text`.
//!
//! Two workflows, one per outbox kind:
//! - `incident_open`: variables `server`, `severity`, `source_ref`, `message`, `link`
//! - `incident_resolve`: variables `server`, `by`, `link`
//!
//! Note: `link` is **not** rendered here. It's pure config (incident_id is
//! already on the row, and `PRIVATE_URL` is operator-set), so the drainer
//! injects it at delivery time. That way only the drainer process needs to
//! see `PRIVATE_URL` — every other process that enqueues outbox rows
//! (public-server, private-server, reachability job) doesn't have to know
//! the operator-facing URL.

use commons_types::issue::Severity;
use serde_json::{Value, json};

use crate::servers::Server;

fn server_label(server: &Server) -> String {
	match &server.name {
		Some(n) if !n.is_empty() => format!("{n} ({})", server.host.0),
		_ => server.host.0.to_string(),
	}
}

fn title_case(s: &str) -> String {
	let mut chars = s.chars();
	match chars.next() {
		Some(c) => c.to_uppercase().chain(chars).collect(),
		None => String::new(),
	}
}

pub fn incident_open(
	server: &Server,
	severity: Severity,
	source: &str,
	issue_ref: &str,
	message: &str,
) -> Value {
	json!({
		"server": server_label(server),
		"severity": title_case(&severity.to_string()),
		"source_ref": format!("{source}/{issue_ref}"),
		"message": message,
	})
}

pub fn incident_resolve(server: &Server, by: Option<&str>) -> Value {
	json!({
		"server": server_label(server),
		"by": by.unwrap_or("automation"),
	})
}
