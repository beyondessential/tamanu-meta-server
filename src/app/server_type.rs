use diesel_derive_enum::DbEnum;
use rocket::{
	http::Header,
	serde::{Deserialize, Serialize},
};
use std::fmt::Display;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, DbEnum)]
#[ExistingTypePath = "crate::schema::sql_types::ServerType"]
#[serde(rename_all = "lowercase")]
pub enum ServerType {
	#[default]
	#[serde(rename = "Tamanu Metadata Server")]
	Meta,
	#[serde(rename = "Tamanu Sync Server")]
	Central,
	#[serde(rename = "Tamanu LAN Server")]
	Facility,
}

#[derive(Debug, Clone, Copy)]
pub struct ServerTypeFromStringError;
impl std::error::Error for ServerTypeFromStringError {}
impl Display for ServerTypeFromStringError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "invalid server type")
	}
}

impl TryFrom<String> for ServerType {
	type Error = ServerTypeFromStringError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		match value.as_ref() {
			"Tamanu Metadata Server" => Ok(Self::Meta),
			"Tamanu Sync Server" => Ok(Self::Central),
			"Tamanu LAN Server" => Ok(Self::Facility),
			_ => Err(ServerTypeFromStringError),
		}
	}
}

impl From<ServerType> for String {
	fn from(server_type: ServerType) -> Self {
		match server_type {
			ServerType::Meta => "Tamanu Metadata Server",
			ServerType::Central => "Tamanu Sync Server",
			ServerType::Facility => "Tamanu LAN Server",
		}
		.into()
	}
}

impl From<ServerType> for Header<'_> {
	fn from(server_type: ServerType) -> Self {
		Header::new("X-Tamanu-Server", String::from(server_type))
	}
}
