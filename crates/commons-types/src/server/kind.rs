use std::{fmt::Display, str::FromStr};

use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

/// What kind of server this is within a deployment.
#[derive(
	Debug,
	Clone,
	Copy,
	Default,
	Eq,
	PartialEq,
	Serialize,
	Deserialize,
	AsExpression,
	utoipa::ToSchema,
)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
	/// A deployment's central server, which its facility servers sync to.
	/// The default.
	#[default]
	Central,
	/// A facility server: an on-site instance that syncs to a central server.
	Facility,
	/// A canopy instance itself, monitored like any other server.
	Canopy,
}

impl Display for ServerKind {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ServerKind::Central => write!(f, "central"),
			ServerKind::Facility => write!(f, "facility"),
			ServerKind::Canopy => write!(f, "canopy"),
		}
	}
}

impl From<ServerKind> for String {
	fn from(rank: ServerKind) -> Self {
		format!("{rank}")
	}
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid server kind: {0}")]
pub struct ServerKindFromStringError(String);

impl FromStr for ServerKind {
	type Err = ServerKindFromStringError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value.to_ascii_lowercase().as_ref() {
			"tamanu sync server" | "central" => Ok(Self::Central),
			"tamanu lan server" | "facility" => Ok(Self::Facility),
			"canopy" => Ok(Self::Canopy),
			s => Err(ServerKindFromStringError(s.into())),
		}
	}
}

impl TryFrom<String> for ServerKind {
	type Error = ServerKindFromStringError;
	fn try_from(value: String) -> Result<Self, Self::Error> {
		value.parse()
	}
}

impl<DB> FromSql<Text, DB> for ServerKind
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		Ok(ServerKind::try_from(s)?)
	}
}

impl ToSql<Text, diesel::pg::Pg> for ServerKind
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}
