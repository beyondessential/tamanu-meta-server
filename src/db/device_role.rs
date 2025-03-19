use std::fmt::Display;

use diesel_derive_enum::DbEnum;
use rocket::serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, DbEnum)]
#[ExistingTypePath = "crate::schema::sql_types::DeviceRole"]
#[serde(rename_all = "lowercase")]
pub enum DeviceRole {
	#[default]
	Untrusted,
	Admin,
	Releaser,
	Server,
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceRoleFromStringError;
impl std::error::Error for DeviceRoleFromStringError {}
impl Display for DeviceRoleFromStringError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "invalid device role")
	}
}

impl TryFrom<String> for DeviceRole {
	type Error = DeviceRoleFromStringError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		match value.to_ascii_lowercase().as_ref() {
			"untrusted" => Ok(Self::Untrusted),
			"admin" => Ok(Self::Admin),
			"releaser" => Ok(Self::Releaser),
			"server" => Ok(Self::Server),
			_ => Err(DeviceRoleFromStringError),
		}
	}
}

impl From<DeviceRole> for String {
	fn from(role: DeviceRole) -> Self {
		match role {
			DeviceRole::Untrusted => "untrusted",
			DeviceRole::Admin => "admin",
			DeviceRole::Releaser => "releaser",
			DeviceRole::Server => "server",
		}
		.into()
	}
}
