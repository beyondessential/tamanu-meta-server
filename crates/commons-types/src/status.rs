use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(
	Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ShortStatus {
	Up,
	Down,
	Away,
	Blip,
	#[default]
	Gone,
}

impl Display for ShortStatus {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ShortStatus::Up => write!(f, "up"),
			ShortStatus::Down => write!(f, "down"),
			ShortStatus::Away => write!(f, "away"),
			ShortStatus::Blip => write!(f, "blip"),
			ShortStatus::Gone => write!(f, "gone"),
		}
	}
}

/// Outcome of a single `health[]` check, as reported by bestool.
///
/// The wire field is `result`; legacy senders use `healthy: bool`
/// instead (exactly one of the two must be present per entry). Use
/// [`CheckResult::from_entry`] to read either form — stored status
/// rows keep the legacy shape forever, so every reader of `health[]`
/// must go through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CheckResult {
	/// Check ran, system under test is fine.
	Passed,
	/// Check ran, system under test is degraded but not failing.
	Warning,
	/// Check ran, system under test is unhealthy.
	Failed,
	/// The check itself errored or is misconfigured; says nothing
	/// about the system under test.
	Broken,
	/// A precondition wasn't met, so the check didn't run.
	Skipped,
}

impl CheckResult {
	/// Normalise a `health[]` entry to its result. Prefers a valid
	/// `result` string, falls back to the legacy `healthy: bool`
	/// (true → Passed, false → Failed). Returns `None` for malformed
	/// entries (neither field readable) — callers ignore those, same
	/// as the historical behaviour for entries missing `healthy`.
	pub fn from_entry(entry: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
		if let Some(result) = entry.get("result").and_then(|v| v.as_str()) {
			return result.parse().ok();
		}
		entry
			.get("healthy")
			.and_then(|v| v.as_bool())
			.map(|healthy| if healthy { Self::Passed } else { Self::Failed })
	}
}

impl Display for CheckResult {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			CheckResult::Passed => write!(f, "passed"),
			CheckResult::Warning => write!(f, "warning"),
			CheckResult::Failed => write!(f, "failed"),
			CheckResult::Broken => write!(f, "broken"),
			CheckResult::Skipped => write!(f, "skipped"),
		}
	}
}

impl std::str::FromStr for CheckResult {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"passed" => Ok(CheckResult::Passed),
			"warning" => Ok(CheckResult::Warning),
			"failed" => Ok(CheckResult::Failed),
			"broken" => Ok(CheckResult::Broken),
			"skipped" => Ok(CheckResult::Skipped),
			other => Err(format!(
				"unknown check result '{other}' (expected passed, warning, failed, broken, or skipped)"
			)),
		}
	}
}

/// One identified human connected to a server right now, distilled from
/// the `external_users` health check on a status push.
///
/// "Identified" means the session's source address resolved to a Tailscale
/// login via `tailscale whois` on the device — local console or
/// non-Tailscale SSH sessions don't produce one of these. One person with
/// several concurrent sessions appears once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OperatorPresence {
	/// Tailscale login (an email), as reported by the device.
	pub login: String,
	/// Display name from the `tailscale_users` cache; `None` when this
	/// login has never authenticated against canopy.
	pub name: Option<String>,
	/// Profile picture URL from the `tailscale_users` cache.
	pub profile_pic: Option<String>,
	/// Earliest `connected_since` across the person's sessions — how long
	/// they've been continuously connected, as tracked by the device.
	pub connected_since: Option<jiff::Timestamp>,
}

/// Distil a status row's `health[]` into the set of identified operators.
///
/// Finds the `external_users` entry and collects its `users[]` sessions
/// that carry a `tailscale` login, deduplicating by login (keeping the
/// earliest `connected_since`). Display fields are left unfilled — looking
/// up the `tailscale_users` cache is the private-server's job. Lenient on
/// shape: malformed entries and sessions are skipped.
pub fn operators_from_health(health: &serde_json::Value) -> Vec<OperatorPresence> {
	let Some(users) = health
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|e| e.as_object())
		.find(|e| e.get("check").and_then(|v| v.as_str()) == Some("external_users"))
		.and_then(|e| e.get("users"))
		.and_then(|u| u.as_array())
	else {
		return Vec::new();
	};

	let mut out: Vec<OperatorPresence> = Vec::new();
	for session in users.iter().filter_map(|u| u.as_object()) {
		let Some(login) = session.get("tailscale").and_then(|v| v.as_str()) else {
			continue;
		};
		let since = session
			.get("connected_since")
			.and_then(|v| v.as_str())
			.and_then(|s| s.parse::<jiff::Timestamp>().ok());
		if let Some(existing) = out.iter_mut().find(|o| o.login == login) {
			existing.connected_since = match (existing.connected_since, since) {
				(Some(a), Some(b)) => Some(a.min(b)),
				(a, b) => a.or(b),
			};
		} else {
			out.push(OperatorPresence {
				login: login.to_string(),
				name: None,
				profile_pic: None,
				connected_since: since,
			});
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::CheckResult;

	fn entry(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
		json.as_object().unwrap().clone()
	}

	#[test]
	fn from_entry_reads_result() {
		for (s, expected) in [
			("passed", CheckResult::Passed),
			("warning", CheckResult::Warning),
			("failed", CheckResult::Failed),
			("broken", CheckResult::Broken),
			("skipped", CheckResult::Skipped),
		] {
			assert_eq!(
				CheckResult::from_entry(&entry(serde_json::json!({"result": s}))),
				Some(expected),
			);
		}
	}

	#[test]
	fn from_entry_falls_back_to_legacy_healthy() {
		assert_eq!(
			CheckResult::from_entry(&entry(serde_json::json!({"healthy": true}))),
			Some(CheckResult::Passed),
		);
		assert_eq!(
			CheckResult::from_entry(&entry(serde_json::json!({"healthy": false}))),
			Some(CheckResult::Failed),
		);
	}

	#[test]
	fn from_entry_result_wins_over_healthy() {
		assert_eq!(
			CheckResult::from_entry(&entry(
				serde_json::json!({"result": "broken", "healthy": true})
			)),
			Some(CheckResult::Broken),
		);
	}

	#[test]
	fn from_entry_malformed_is_none() {
		assert_eq!(CheckResult::from_entry(&entry(serde_json::json!({}))), None);
		assert_eq!(
			CheckResult::from_entry(&entry(serde_json::json!({"result": "exploded"}))),
			None,
		);
		assert_eq!(
			CheckResult::from_entry(&entry(serde_json::json!({"healthy": "yes"}))),
			None,
		);
	}

	mod operators {
		use crate::status::operators_from_health;

		#[test]
		fn collects_tailscale_logins_in_order() {
			let health = serde_json::json!([
				{"check": "postgres", "result": "passed"},
				{"check": "external_users", "result": "passed", "count": 2, "users": [
					{"name": "ubuntu", "line": "pts/0", "tailscale": "alice@example.com",
					 "connected_since": "2026-06-01T03:56:40.073731977Z"},
					{"name": "ubuntu", "line": "pts/1", "tailscale": "bob@example.com",
					 "connected_since": "2026-06-01T04:01:53Z"},
				]},
			]);
			let ops = operators_from_health(&health);
			assert_eq!(ops.len(), 2);
			assert_eq!(ops[0].login, "alice@example.com");
			assert_eq!(ops[1].login, "bob@example.com");
			assert_eq!(
				ops[0].connected_since,
				Some("2026-06-01T03:56:40.073731977Z".parse().unwrap()),
			);
			assert!(ops[0].name.is_none());
			assert!(ops[0].profile_pic.is_none());
		}

		#[test]
		fn dedupes_by_login_keeping_earliest_since() {
			// Same person on three ttys; the middle session has the
			// earliest connected_since and must win regardless of order.
			let health = serde_json::json!([
				{"check": "external_users", "result": "passed", "users": [
					{"line": "pts/3", "tailscale": "chris@example.com",
					 "connected_since": "2026-06-02T03:26:00Z"},
					{"line": "pts/8", "tailscale": "chris@example.com",
					 "connected_since": "2026-06-01T06:58:49Z"},
					{"line": "pts/9", "tailscale": "chris@example.com",
					 "connected_since": "2026-06-04T23:47:00Z"},
				]},
			]);
			let ops = operators_from_health(&health);
			assert_eq!(ops.len(), 1);
			assert_eq!(
				ops[0].connected_since,
				Some("2026-06-01T06:58:49Z".parse().unwrap()),
			);
		}

		#[test]
		fn skips_sessions_without_tailscale_identity() {
			let health = serde_json::json!([
				{"check": "external_users", "result": "passed", "users": [
					{"name": "root", "line": "tty1"},
					{"name": "ubuntu", "line": "pts/0", "source": "203.0.113.5"},
					{"name": "ubuntu", "line": "pts/1", "tailscale": "alice@example.com"},
				]},
			]);
			let ops = operators_from_health(&health);
			assert_eq!(ops.len(), 1);
			assert_eq!(ops[0].login, "alice@example.com");
			assert_eq!(ops[0].connected_since, None);
		}

		#[test]
		fn unparseable_since_does_not_drop_the_operator() {
			let health = serde_json::json!([
				{"check": "external_users", "result": "passed", "users": [
					{"tailscale": "alice@example.com", "connected_since": "yesterday-ish"},
					{"tailscale": "alice@example.com", "connected_since": "2026-06-01T06:58:49Z"},
				]},
			]);
			let ops = operators_from_health(&health);
			assert_eq!(ops.len(), 1);
			assert_eq!(
				ops[0].connected_since,
				Some("2026-06-01T06:58:49Z".parse().unwrap()),
			);
		}

		#[test]
		fn tolerates_absent_or_malformed_shapes() {
			for health in [
				serde_json::json!(null),
				serde_json::json!([]),
				serde_json::json!([{"check": "postgres", "result": "passed"}]),
				serde_json::json!([{"check": "external_users", "result": "skipped"}]),
				serde_json::json!([{"check": "external_users", "users": "not-an-array"}]),
				serde_json::json!([{"check": "external_users", "users": [42, "str", null]}]),
			] {
				assert_eq!(operators_from_health(&health), Vec::new(), "for {health}");
			}
		}
	}
}

/// Server's self-reported health state, derived from the most
/// recent status row's per-check results (and, as legacy input, its
/// top-level `healthy` field — that flag is being retired from the
/// wire). Orthogonal to [`ShortStatus`]: a server can be reachable
/// (`up`) and reporting itself unhealthy at the same time.
///
/// The UI renders this as the *border* of `<StatusDot>` so both
/// dimensions (reachability and self-report) show in one glyph.
#[derive(
	Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
	/// Every `health[]` entry passed or was skipped (and the legacy
	/// top-level `healthy` flag, if sent, was `true`). Also the
	/// default for servers with no status row at all — there's no
	/// signal that says otherwise. (Reachability covers the "we
	/// haven't heard from this server" case separately.)
	#[default]
	Healthy,
	/// At least one `health[]` entry reports warning or broken — or,
	/// in the legacy form, `healthy: false` under top-level `true`
	/// (old bestool's warning encoding). Operator should investigate
	/// but it's not an incident.
	Warning,
	/// At least one `health[]` entry reports `result: failed`, or the
	/// legacy top-level `healthy` flag was `false`. Incident-class.
	Unhealthy,
}
