#[cfg(feature = "ssr")]
use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(AsExpression))]
#[cfg_attr(feature = "ssr", diesel(sql_type = Text))]
#[serde(rename_all = "lowercase")]
pub enum DeviceRole {
	#[default]
	Untrusted,
	Admin,
	Releaser,
	Server,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid device role")]
pub struct DeviceRoleFromStringError;

impl std::str::FromStr for DeviceRole {
	type Err = DeviceRoleFromStringError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_ascii_lowercase().as_ref() {
			"untrusted" => Ok(Self::Untrusted),
			"admin" => Ok(Self::Admin),
			"releaser" => Ok(Self::Releaser),
			"server" => Ok(Self::Server),
			_ => Err(DeviceRoleFromStringError),
		}
	}
}

impl TryFrom<String> for DeviceRole {
	type Error = DeviceRoleFromStringError;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		value.parse()
	}
}

impl std::fmt::Display for DeviceRole {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let s = match self {
			DeviceRole::Untrusted => "untrusted",
			DeviceRole::Admin => "admin",
			DeviceRole::Releaser => "releaser",
			DeviceRole::Server => "server",
		};
		write!(f, "{}", s)
	}
}

impl From<DeviceRole> for String {
	fn from(role: DeviceRole) -> Self {
		role.to_string()
	}
}

crate::macros::render_as_string!(DeviceRole, minsize(5));

#[cfg(feature = "ssr")]
impl<DB> FromSql<Text, DB> for DeviceRole
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		Ok(DeviceRole::try_from(s)?)
	}
}

#[cfg(feature = "ssr")]
impl ToSql<Text, diesel::pg::Pg> for DeviceRole
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}
