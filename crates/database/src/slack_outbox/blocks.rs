//! Block Kit payload renderers for slack-outbox rows.
//!
//! Each `render_*` function returns the `blocks` array as a `serde_json::Value`
//! ready to drop into a webhook body (Phase A) or a `chat.postMessage` call
//! (Phase B). Rendering happens at *enqueue* time so the message reflects the
//! state when the event fired, not whatever it happens to look like by the
//! time the worker drains the row.

use commons_types::issue::Severity;
use serde_json::{Value, json};

use crate::{issues::Incident, servers::Server};

fn severity_emoji(sev: Severity) -> &'static str {
	match sev {
		Severity::Emergency | Severity::Alert | Severity::Critical => "🔥",
		Severity::Error => "🚨",
		Severity::Warning => "⚠️",
		Severity::Notice => "📣",
		Severity::Info | Severity::Debug => "ℹ️",
	}
}

fn incident_link(public_url: Option<&str>, incident_id: uuid::Uuid) -> Option<String> {
	let base = public_url?.trim_end_matches('/');
	Some(format!("{base}/incidents/{incident_id}"))
}

fn server_label(server: &Server) -> String {
	match &server.name {
		Some(n) if !n.is_empty() => format!("{n} ({})", server.host.0),
		_ => server.host.0.to_string(),
	}
}

/// Top-level "incident opened" message.
///
/// Takes the highest severity among the joining issues — at incident open
/// time that's whatever issue triggered the open.
pub fn incident_open(
	incident: &Incident,
	server: &Server,
	severity: Severity,
	source: &str,
	issue_ref: &str,
	message: &str,
	public_url: Option<&str>,
) -> Value {
	let emoji = severity_emoji(severity);
	let mut blocks = vec![
		json!({
			"type": "header",
			"text": {
				"type": "plain_text",
				"text": format!("{emoji} Incident opened — {}", server_label(server)),
				"emoji": true,
			},
		}),
		json!({
			"type": "section",
			"fields": [
				{ "type": "mrkdwn", "text": format!("*Severity*\n{severity}") },
				{ "type": "mrkdwn", "text": format!("*Source*\n`{source}` / `{issue_ref}`") },
			],
		}),
		json!({
			"type": "section",
			"text": { "type": "mrkdwn", "text": message },
		}),
	];
	if let Some(link) = incident_link(public_url, incident.id) {
		blocks.push(json!({
			"type": "context",
			"elements": [
				{ "type": "mrkdwn", "text": format!("<{link}|Open in canopy>") },
			],
		}));
	}
	Value::Array(blocks)
}

/// Top-level "incident resolved" message (Phase A). Phase B will reuse this
/// renderer but post it as a thread reply instead.
pub fn incident_resolve(
	incident: &Incident,
	server: &Server,
	by: Option<&str>,
	public_url: Option<&str>,
) -> Value {
	let who = by.unwrap_or("automation");
	let mut blocks = vec![
		json!({
			"type": "header",
			"text": {
				"type": "plain_text",
				"text": format!("✅ Incident resolved — {}", server_label(server)),
				"emoji": true,
			},
		}),
		json!({
			"type": "context",
			"elements": [
				{ "type": "mrkdwn", "text": format!("Resolved by *{who}*.") },
			],
		}),
	];
	if let Some(link) = incident_link(public_url, incident.id) {
		blocks.push(json!({
			"type": "context",
			"elements": [
				{ "type": "mrkdwn", "text": format!("<{link}|Open in canopy>") },
			],
		}));
	}
	Value::Array(blocks)
}
