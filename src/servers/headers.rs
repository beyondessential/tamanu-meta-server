use axum::{extract::FromRequestParts, http::request::Parts};

use crate::error::AppError;

use super::version::VersionStr;

const X_VERSION: &str = "X-Version";
const TAILSCALE_USER_NAME: &str = "Tailscale-User-Name";

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

#[derive(Debug, Clone)]
pub struct TailscaleUserName(pub Option<String>);

impl<S> FromRequestParts<S> for TailscaleUserName
where
	S: Send + Sync,
{
	type Rejection = std::convert::Infallible;

	async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
		let user_name = parts.headers.get(TAILSCALE_USER_NAME).and_then(|value| {
			rfc2047_decoder::decode(value.as_bytes())
				.ok()
				.or_else(|| value.to_str().ok().map(ToOwned::to_owned))
		});

		Ok(TailscaleUserName(user_name))
	}
}
