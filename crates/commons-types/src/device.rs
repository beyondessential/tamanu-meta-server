use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

/// The role a device is trusted with, which determines what it may do.
#[derive(
	Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsExpression, utoipa::ToSchema,
)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "lowercase")]
pub enum DeviceRole {
	/// Full administrative access, including everything the other roles can do.
	Admin,
	/// May publish release versions and register their artifacts.
	Releaser,
	/// Acts as a monitored server: submits statuses and events, runs backups.
	Server,
	/// May run managed restores of backups onto replica servers.
	#[serde(rename = "backup-restore")]
	BackupRestore,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid device role")]
pub struct DeviceRoleFromStringError;

impl std::str::FromStr for DeviceRole {
	type Err = DeviceRoleFromStringError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_ascii_lowercase().as_ref() {
			"admin" => Ok(Self::Admin),
			"releaser" => Ok(Self::Releaser),
			"server" => Ok(Self::Server),
			"backup-restore" => Ok(Self::BackupRestore),
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
			DeviceRole::Admin => "admin",
			DeviceRole::Releaser => "releaser",
			DeviceRole::Server => "server",
			DeviceRole::BackupRestore => "backup-restore",
		};
		write!(f, "{}", s)
	}
}

impl From<DeviceRole> for String {
	fn from(role: DeviceRole) -> Self {
		role.to_string()
	}
}

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

impl ToSql<Text, diesel::pg::Pg> for DeviceRole
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}
