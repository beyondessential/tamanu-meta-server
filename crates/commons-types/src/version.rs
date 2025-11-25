use std::{fmt::Display, str::FromStr};

use commons_errors::AppError;
#[cfg(feature = "ssr")]
use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use node_semver::SemverError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(AsExpression))]
#[cfg_attr(feature = "ssr", diesel(sql_type = Text))]
#[serde(rename_all = "lowercase")]
pub enum VersionStatus {
	Draft,
	Published,
	Yanked,
}

impl Display for VersionStatus {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			VersionStatus::Draft => write!(f, "draft"),
			VersionStatus::Published => write!(f, "published"),
			VersionStatus::Yanked => write!(f, "yanked"),
		}
	}
}

impl Default for VersionStatus {
	fn default() -> Self {
		Self::Draft
	}
}

impl From<String> for VersionStatus {
	fn from(value: String) -> Self {
		match value.to_ascii_lowercase().as_ref() {
			"draft" => Self::Draft,
			"published" => Self::Published,
			"yanked" => Self::Yanked,
			_ => Self::default(),
		}
	}
}

impl From<VersionStatus> for String {
	fn from(status: VersionStatus) -> Self {
		match status {
			VersionStatus::Draft => "draft",
			VersionStatus::Published => "published",
			VersionStatus::Yanked => "yanked",
		}
		.into()
	}
}

#[cfg(feature = "ssr")]
impl<DB> FromSql<Text, DB> for VersionStatus
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		Ok(VersionStatus::from(s))
	}
}

#[cfg(feature = "ssr")]
impl ToSql<Text, diesel::pg::Pg> for VersionStatus
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(AsExpression))]
#[cfg_attr(feature = "ssr", diesel(sql_type = Text))]
pub struct VersionStr(pub node_semver::Version);

impl Display for VersionStr {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl Default for VersionStr {
	fn default() -> Self {
		Self(node_semver::Version::new(0, 0, 0))
	}
}

impl FromStr for VersionStr {
	type Err = AppError;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Ok(Self(if let Some(v) = s.strip_prefix("v") {
			node_semver::Version::parse(v)?
		} else {
			node_semver::Version::parse(s)?
		}))
	}
}

#[cfg(feature = "ssr")]
impl<DB> deserialize::FromSql<Text, DB> for VersionStr
where
	DB: Backend,
	String: deserialize::FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		String::from_sql(bytes).and_then(|string| {
			node_semver::Version::parse(string)
				.map(VersionStr)
				.map_err(|err| Box::new(err) as Box<dyn std::error::Error + Send + Sync>)
		})
	}
}

#[cfg(feature = "ssr")]
impl serialize::ToSql<Text, diesel::pg::Pg> for VersionStr
where
	String: serialize::ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(
		&'b self,
		out: &mut serialize::Output<'b, '_, diesel::pg::Pg>,
	) -> serialize::Result {
		let v = self.0.to_string();
		<String as serialize::ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct VersionRange(pub node_semver::Range);

impl FromStr for VersionRange {
	type Err = SemverError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		if let Some(rest) = s.strip_prefix("^") {
			// TEMPORARY WORKAROUND for a bad default parameter in Tamanu
			format!("~{rest}").parse().map(Self)
		} else if let Some(v) = s.strip_prefix("v") {
			v.parse().map(Self)
		} else {
			s.parse().map(Self)
		}
	}
}
