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

#[cfg(test)]
mod tests {
	use super::*;
	use axum::{Router, routing::get};

	async fn handler(v: Option<VersionHeader>) -> String {
		format!("{:?}", v.map(|v| v.0.to_string()))
	}

	#[tokio::test]
	async fn a_malformed_version_header_rejects_rather_than_reading_as_absent() {
		let app = Router::new().route("/", get(handler));
		let server = axum_test::TestServer::new(app);

		assert_eq!(
			server.get("/").await.status_code().as_u16(),
			200,
			"absent is fine"
		);
		assert_eq!(
			server
				.get("/")
				.add_header("x-version", "2.3.4")
				.await
				.status_code()
				.as_u16(),
			200,
			"a good version is fine",
		);
		assert_eq!(
			server
				.get("/")
				.add_header("x-version", "garbage")
				.await
				.status_code()
				.as_u16(),
			400,
			"a garbage version is a client bug, not an absent header",
		);
	}
}
