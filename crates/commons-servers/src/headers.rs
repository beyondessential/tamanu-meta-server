use axum::{
	extract::{FromRequestParts, OptionalFromRequestParts},
	http::request::Parts,
};
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

impl<S> OptionalFromRequestParts<S> for VersionHeader
where
	S: Send + Sync,
{
	type Rejection = AppError;

	/// `None` when the header is absent — the version is now sourced
	/// primarily from the status payload's `tamanuVersion`, so a sender
	/// that omits `X-Version` is no longer an error. A present-but-malformed
	/// header still rejects (a garbage version is a client bug worth surfacing).
	async fn from_request_parts(
		parts: &mut Parts,
		state: &S,
	) -> Result<Option<Self>, Self::Rejection> {
		if parts.headers.get(X_VERSION).is_none() {
			return Ok(None);
		}
		<Self as FromRequestParts<S>>::from_request_parts(parts, state)
			.await
			.map(Some)
	}
}
