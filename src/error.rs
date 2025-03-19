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
}

impl AppError {
	pub fn custom(err: impl ToString) -> Self {
		Self::Custom(err.to_string())
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
			.status(Status::InternalServerError)
			.sized_body(json.len(), Cursor::new(json))
			.finalize())
	}
}
