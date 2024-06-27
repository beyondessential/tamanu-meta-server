use rocket::serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlField(pub Url);

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
