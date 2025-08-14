use std::str::FromStr;

use diesel::{backend::Backend, deserialize, expression::AsExpression, serialize, sql_types::Text};
use node_semver::SemverError;
use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, AsExpression)]
#[diesel(sql_type = Text)]
pub struct VersionStr(pub node_semver::Version);

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
