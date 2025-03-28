use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use rocket::serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, AsExpression)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
	#[default]
	Central,
	Facility,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid server kind: {0}")]
pub struct ServerKindFromStringError(String);

impl TryFrom<String> for ServerKind {
	type Error = ServerKindFromStringError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		match value.to_ascii_lowercase().as_ref() {
			"tamanu sync server" | "central" => Ok(Self::Central),
			"tamanu lan server" | "facility" => Ok(Self::Facility),
			s => Err(ServerKindFromStringError(s.into())),
		}
	}
}

impl From<ServerKind> for String {
	fn from(rank: ServerKind) -> Self {
		match rank {
			ServerKind::Central => "central",
			ServerKind::Facility => "facility",
		}
		.into()
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
