use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

crate::macros::render_as_string!(ShortStatus, minsize(2));
