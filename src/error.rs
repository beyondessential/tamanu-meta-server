use std::env::VarError;

use axum::{
	Json,
	http::StatusCode,
	response::{IntoResponse, Response},
};
use diesel_async::pooled_connection::PoolError;
use serde::Serialize;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error, miette::Diagnostic, Serialize)]
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

	#[error("render: {0}")]
	#[serde(serialize_with = "serialize_to_string")]
	Tera(#[from] tera::Error),

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

pub fn serialize_to_string<E: ToString, S>(
	value: &E,
	serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	value.to_string().serialize(serializer)
}

impl IntoResponse for AppError {
	fn into_response(self) -> Response {
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
