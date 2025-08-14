use crate::error::AppError;

use super::version::Version;
use rocket::{
	Request,
	http::Status,
	request::{FromParam, FromRequest, Outcome},
};

const X_VERSION: &str = "X-Version";

#[derive(Debug, Clone)]
pub struct VersionHeader(pub Version);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for VersionHeader {
	type Error = ();

	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		match request.headers().get_one(X_VERSION) {
			Some(value) => match Version::from_param(value) {
				Ok(version) => Outcome::Success(VersionHeader(version)),
				Err(_) => Outcome::Forward(Status::BadRequest),
			},
			None => Outcome::Forward(Status::BadRequest),
		}
	}
}

impl<S> axum::extract::FromRequestParts<S> for VersionHeader
where
	S: Send + Sync,
{
	type Rejection = AppError;

	async fn from_request_parts(
		parts: &mut axum::http::request::Parts,
		_: &S,
	) -> Result<Self, Self::Rejection> {
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
