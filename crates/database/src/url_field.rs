use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Deserialize)]
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
