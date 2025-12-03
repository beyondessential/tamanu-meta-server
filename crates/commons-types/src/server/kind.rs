use std::{fmt::Display, str::FromStr};

#[cfg(feature = "ssr")]
use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(AsExpression))]
#[cfg_attr(feature = "ssr", diesel(sql_type = Text))]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
	#[default]
	Central,
	Facility,
	Meta,
}

impl Display for ServerKind {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ServerKind::Central => write!(f, "central"),
			ServerKind::Facility => write!(f, "facility"),
			ServerKind::Meta => write!(f, "meta"),
		}
	}
}

impl From<ServerKind> for String {
	fn from(rank: ServerKind) -> Self {
		format!("{rank}")
	}
}

commons_macros::render_as_string!(ServerKind, minsize(4));

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid server kind: {0}")]
pub struct ServerKindFromStringError(String);

impl FromStr for ServerKind {
	type Err = ServerKindFromStringError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value.to_ascii_lowercase().as_ref() {
			"tamanu sync server" | "central" => Ok(Self::Central),
			"tamanu lan server" | "facility" => Ok(Self::Facility),
			"meta" => Ok(Self::Meta),
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

#[cfg(feature = "ssr")]
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

#[cfg(feature = "ssr")]
impl ToSql<Text, diesel::pg::Pg> for ServerKind
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}
