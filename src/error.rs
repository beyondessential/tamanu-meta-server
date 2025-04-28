use std::io::Cursor;

use rocket::{
	http::{ContentType, Status},
	response::Responder,
	Response,
};
use serde::Serialize;

pub type Result<T, E = AppError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, Serialize)]
pub enum AppError {
	#[error("{0}")]
	Custom(String),

	// it's practically impossible to wrangle rocket's actual db error here, so string it
	#[error("database: {0}")]
	Database(String),

	#[error("io: {0}")]
	Io(String),

	#[error("no versions match given range")]
	NoMatchingVersions,

	#[error("version range is not usable")]
	UnusableRange,

	#[error("timesync: {0}")]
	#[serde(serialize_with = "serialize_timesimp_error")]
	Timesync(#[from] timesimp::ParseError),
}

impl AppError {
	pub fn custom(err: impl ToString) -> Self {
		Self::Custom(err.to_string())
	}

	pub fn database(err: impl ToString) -> Self {
		Self::Database(err.to_string())
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

pub fn serialize_timesimp_error<S>(
	value: &timesimp::ParseError,
	serializer: S,
) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	value.to_string().serialize(serializer)
}
