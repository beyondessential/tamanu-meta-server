use std::io::Write;

use diesel::{
	deserialize::{self, FromSql, FromSqlRow},
	expression::AsExpression,
	pg::{Pg, PgValue},
	serialize::{self, IsNull, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Deserialize, AsExpression, FromSqlRow, utoipa::ToSchema)]
#[diesel(sql_type = Text)]
#[schema(value_type = String, format = "uri")]
pub struct UrlField(pub Url);

impl Serialize for UrlField {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		// always serialize without trailing slash as mobiles don't like it
		let s = self.0.to_string();
		let s = s.strip_suffix('/').unwrap_or(&s);
		s.serialize(serializer)
	}
}

impl TryFrom<String> for UrlField {
	type Error = url::ParseError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		Ok(Self(Url::parse(&value)?))
	}
}

impl From<UrlField> for String {
	fn from(url: UrlField) -> Self {
		url.0.to_string()
	}
}

impl ToSql<Text, Pg> for UrlField {
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
		out.write_all(self.0.as_str().as_bytes())?;
		Ok(IsNull::No)
	}
}

impl FromSql<Text, Pg> for UrlField {
	fn from_sql(bytes: PgValue<'_>) -> deserialize::Result<Self> {
		let s = <String as FromSql<Text, Pg>>::from_sql(bytes)?;
		Ok(Self(Url::parse(&s)?))
	}
}
