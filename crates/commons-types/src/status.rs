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
			"failed" => Ok(CheckResult::Failed),
			"broken" => Ok(CheckResult::Broken),
			"skipped" => Ok(CheckResult::Skipped),
			other => Err(format!(
				"unknown check result '{other}' (expected passed, failed, broken, or skipped)"
			)),
		}
	}
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
}

/// Server's self-reported health state, derived from the most
/// recent status row's `healthy` field and `health[]` array.
/// Orthogonal to [`ShortStatus`]: a server can be reachable
/// (`up`) and reporting itself unhealthy at the same time.
///
/// The UI renders this as the *border* of `<StatusDot>` so both
/// dimensions (reachability and self-report) show in one glyph.
#[derive(
	Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
	/// Top-level `healthy: true` and every entry in `health[]` is
	/// also healthy. Also the default for servers with no status row
	/// at all — there's no signal that says otherwise. (Reachability
	/// covers the "we haven't heard from this server" case
	/// separately.)
	#[default]
	Healthy,
	/// Top-level `healthy: true` but at least one `health[]` entry
	/// reports `healthy: false`. Operator should investigate but
	/// it's not an incident.
	Warning,
	/// Top-level `healthy: false`. Incident-class.
	Unhealthy,
}
