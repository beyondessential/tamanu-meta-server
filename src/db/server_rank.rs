use std::fmt::Display;

use rocket::serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerRank {
	Live,
	Demo,
	Dev,
}

#[derive(Debug, Clone, Copy)]
pub struct ServerRankFromStringError;
impl std::error::Error for ServerRankFromStringError {}
impl Display for ServerRankFromStringError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "invalid server rank")
	}
}

impl TryFrom<String> for ServerRank {
	type Error = ServerRankFromStringError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		match value.to_ascii_lowercase().as_ref() {
			"live" => Ok(Self::Live),
			"demo" => Ok(Self::Demo),
			"dev" => Ok(Self::Dev),
			_ => Err(ServerRankFromStringError),
		}
	}
}

impl From<ServerRank> for String {
	fn from(rank: ServerRank) -> Self {
		match rank {
			ServerRank::Live => "live",
			ServerRank::Demo => "demo",
			ServerRank::Dev => "dev",
		}
		.into()
	}
}
