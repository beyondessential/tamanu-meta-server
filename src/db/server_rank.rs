use std::fmt::Display;

use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

#[derive(
	Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Serialize, Deserialize, AsExpression,
)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "lowercase")]
pub enum ServerRank {
	Production,
	Clone,
	Demo,
	Test,
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
			"live" | "prod" | "production" => Ok(Self::Production),
			"clone" | "staging" => Ok(Self::Clone),
			"demo" => Ok(Self::Demo),
			"test" => Ok(Self::Test),
			"dev" => Ok(Self::Dev),
			_ => Err(ServerRankFromStringError),
		}
	}
}

impl From<ServerRank> for String {
	fn from(rank: ServerRank) -> Self {
		match rank {
			ServerRank::Production => "production",
			ServerRank::Clone => "clone",
			ServerRank::Demo => "demo",
			ServerRank::Test => "test",
			ServerRank::Dev => "dev",
		}
		.into()
	}
}

impl<DB> FromSql<Text, DB> for ServerRank
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		ServerRank::try_from(s.clone()).map_err(|_| format!("Unrecognized variant {}", s).into())
	}
}

impl ToSql<Text, diesel::pg::Pg> for ServerRank
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}
