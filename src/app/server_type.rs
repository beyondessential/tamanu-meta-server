use diesel_derive_enum::DbEnum;
use rocket::{
	http::Header,
	serde::{Deserialize, Serialize},
};

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
