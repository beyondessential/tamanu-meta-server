use axum::{extract::FromRequestParts, http::request::Parts};
use commons_errors::AppError;
use commons_types::version::VersionStr;

const X_VERSION: &str = "X-Version";

#[derive(Debug, Clone)]
pub struct VersionHeader(pub VersionStr);

impl<S> FromRequestParts<S> for VersionHeader
where
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
		let param = parts
			.headers
			.get(X_VERSION)
			.ok_or_else(|| AppError::Header(format!("missing {X_VERSION}")))?
			.to_str()
			.map_err(|err| AppError::Header(err.to_string()))?
			.parse()?;

		Ok(VersionHeader(param))
	}
}
