use std::{env::VarError, io::Cursor};

use diesel_async::pooled_connection::PoolError;
use rocket::{
	Response,
	http::{ContentType, Status},
	response::Responder,
};
use serde::Serialize;

pub type Result<T, E = AppError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, Serialize)]
pub enum AppError {
	#[error("{0}")]
	Custom(String),

	#[error("environment: {0}")]
	#[serde(serialize_with = "serialize_to_string")]
	Environment(#[from] VarError),

	#[error("header: {0}")]
	Header(String),

	#[error("version parse error: {0}")]
	#[serde(serialize_with = "serialize_to_string")]
	VersionParse(#[from] node_semver::SemverError),

	#[error("database: {0}")]
	#[serde(serialize_with = "serialize_to_string")]
	DatabasePool(#[from] mobc::Error<PoolError>),

	#[error("database: {0}")]
	#[serde(serialize_with = "serialize_to_string")]
	DatabaseQuery(#[from] diesel::result::Error),

	#[error("io: {0}")]
	Io(String),

	#[error("no versions match given range")]
	NoMatchingVersions,

	#[error("version range is not usable")]
	UnusableRange,

	#[error("timesync: {0}")]
	#[serde(serialize_with = "serialize_to_string")]
	Timesync(#[from] timesimp::ParseError),
}

impl AppError {
	pub fn custom(err: impl ToString) -> Self {
		Self::Custom(err.to_string())
	}
}

impl From<std::io::Error> for AppError {
	fn from(err: std::io::Error) -> Self {
		Self::Io(err.to_string())
	}
}

impl<'r, 'o: 'r> Responder<'r, 'o> for AppError {
	fn respond_to(self, _request: &'r rocket::Request<'_>) -> rocket::response::Result<'o> {
		let json = serde_json::to_string_pretty(&self).map_err(|err| {
			error!("failed to serialize error: {err}");
			Status::InternalServerError
		})?;

		Ok(Response::build()
			.header(ContentType::JSON)
			.status(match self {
				Self::NoMatchingVersions => Status::NotFound,
				Self::UnusableRange => Status::BadRequest,
				_ => Status::InternalServerError,
			})
			.sized_body(json.len(), Cursor::new(json))
			.finalize())
	}
}

pub fn serialize_to_string<E: ToString, S>(value: &E, serializer: S) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	value.to_string().serialize(serializer)
}

impl axum::response::IntoResponse for AppError {
	fn into_response(self) -> axum::response::Response {
		use axum::{Json, http::StatusCode};

		let status = match self {
			Self::NoMatchingVersions => StatusCode::NOT_FOUND,
			Self::UnusableRange => StatusCode::BAD_REQUEST,
			_ => StatusCode::INTERNAL_SERVER_ERROR,
		};

		let mut res = Json(self).into_response();
		*res.status_mut() = status;
		res
	}
}
