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
