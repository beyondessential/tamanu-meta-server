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

use commons_types::issue::Severity;
use serde_json::{Value, json};

use crate::{issues::Incident, servers::Server};

/// `link` always returns a well-formed URL. If `PUBLIC_URL` is unset the
/// caller can pass `None` and we fall back to a localhost placeholder — that
/// way the `<{{link}}|Open in canopy>` mrkdwn in the workflow editor still
/// renders as a clickable (broken) link in dev rather than as malformed text
/// in prod. Set `PUBLIC_URL` in any environment that posts to a real Slack.
fn incident_link(public_url: Option<&str>, incident_id: uuid::Uuid) -> String {
	let base = public_url
		.unwrap_or("http://localhost")
		.trim_end_matches('/');
	format!("{base}/incidents/{incident_id}")
}

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
	incident: &Incident,
	server: &Server,
	severity: Severity,
	source: &str,
	issue_ref: &str,
	message: &str,
	public_url: Option<&str>,
) -> Value {
	json!({
		"server": server_label(server),
		"severity": title_case(&severity.to_string()),
		"source_ref": format!("{source}/{issue_ref}"),
		"message": message,
		"link": incident_link(public_url, incident.id),
	})
}

pub fn incident_resolve(
	incident: &Incident,
	server: &Server,
	by: Option<&str>,
	public_url: Option<&str>,
) -> Value {
	json!({
		"server": server_label(server),
		"by": by.unwrap_or("automation"),
		"link": incident_link(public_url, incident.id),
	})
}
