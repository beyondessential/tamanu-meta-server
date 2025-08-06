use diesel::{backend::Backend, deserialize, expression::AsExpression, serialize, sql_types::Text};
use node_semver::SemverError;
use rocket::{http::Header, request::FromParam, serde::Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, AsExpression)]
#[diesel(sql_type = Text)]
pub struct Version(pub node_semver::Version);

impl Default for Version {
	fn default() -> Self {
		Self(node_semver::Version::new(0, 0, 0))
	}
}

impl From<Version> for Header<'_> {
	fn from(version: Version) -> Self {
		Header::new("X-Version", version.0.to_string())
	}
}

impl FromParam<'_> for Version {
	type Error = SemverError;

	fn from_param(param: &'_ str) -> Result<Self, Self::Error> {
		if let Some(v) = param.strip_prefix("v") {
			v.parse().map(Self)
		} else {
			param.parse().map(Self)
		}
	}
}

impl<DB> deserialize::FromSql<Text, DB> for Version
where
	DB: Backend,
	String: deserialize::FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		String::from_sql(bytes).and_then(|string| {
			node_semver::Version::parse(string)
				.map(Version)
				.map_err(|err| Box::new(err) as Box<dyn std::error::Error + Send + Sync>)
		})
	}
}

impl serialize::ToSql<Text, diesel::pg::Pg> for Version
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

impl FromParam<'_> for VersionRange {
	type Error = SemverError;

	fn from_param(param: &'_ str) -> Result<Self, Self::Error> {
		if let Some(rest) = param.strip_prefix("^") {
			// TEMPORARY WORKAROUND for a bad default parameter in Tamanu
			format!("~{rest}").parse().map(Self)
		} else if let Some(v) = param.strip_prefix("v") {
			v.parse().map(Self)
		} else {
			param.parse().map(Self)
		}
	}
}
