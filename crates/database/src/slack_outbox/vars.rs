//! Flat-variable payloads for Slack Workflow Builder webhook triggers.
//!
//! Workflow Builder webhooks don't accept Block Kit directly. Each workflow
//! declares a named set of text variables in its trigger; the actual message
//! is composed inside Slack's workflow editor using those variables. So we
//! POST a flat JSON object keyed by the declared variable names — no
//! `blocks`, no `text`.
//!
//! Three workflows:
//! - `incident_open`: variables `server`, `severity`, `source_ref`, `message`, `link`
//! - `incident_resolve`: variables `server`, `by`, `link`. `by` is either the
//!   resolving operator's login, or — when the incident retired on its own —
//!   "the healthcheck recovering" (it describes the event, not an actor: the
//!   check went healthy again, which does not imply nobody intervened).
//! - self-alert (both `self_alert_open` and `self_alert_resolve` kinds):
//!   variables `state`, `severity`, `source_ref`, `title`, `message`, `link` —
//!   one workflow serves raise and recovery, told apart by `state`.
//!
//! Note: `link` is **not** rendered here. It's pure config (incident_id is
//! already on the row, and `PRIVATE_URL` is operator-set), so the drainer
//! injects it at delivery time. That way only the drainer process needs to
//! see `PRIVATE_URL` — every other process that enqueues outbox rows
//! (public-server, private-server, reachability job) doesn't have to know
//! the operator-facing URL.

use commons_types::issue::Severity;
use serde_json::{Value, json};

fn title_case(s: &str) -> String {
	let mut chars = s.chars();
	match chars.next() {
		Some(c) => c.to_uppercase().chain(chars).collect(),
		None => String::new(),
	}
}

pub fn incident_open(
	server_label: &str,
	severity: Severity,
	source: &str,
	issue_ref: &str,
	message: &str,
) -> Value {
	json!({
		"server": server_label,
		"severity": title_case(&severity.to_string()),
		"source_ref": format!("{source}/{issue_ref}"),
		"message": message,
	})
}

/// Self-alert raise. `title` is the issue's single-line description.
pub fn self_alert_open(severity: Severity, issue_ref: &str, title: &str, message: &str) -> Value {
	json!({
		"state": "alert",
		"severity": title_case(&severity.to_string()),
		"source_ref": format!("canopy/{issue_ref}"),
		"title": title,
		"message": message,
	})
}

/// Self-alert recovery. Same variable set as the raise so one Workflow
/// Builder workflow serves both.
pub fn self_alert_resolve(issue_ref: &str, message: &str) -> Value {
	json!({
		"state": "recovered",
		"severity": "Info",
		"source_ref": format!("canopy/{issue_ref}"),
		"title": "recovered",
		"message": message,
	})
}

pub fn incident_resolve(server_label: &str, by: Option<&str>) -> Value {
	json!({
		"server": server_label,
		// `None` means no operator was attached to the close: the incident
		// retired because its healthcheck started reporting healthy again.
		// Phrase it as the triggering event, not an actor — "automation"
		// was read as "a bot decided to close this", when in practice an
		// operator was usually involved; canopy just can't attribute it.
		"by": by.unwrap_or("the healthcheck recovering"),
	})
}
