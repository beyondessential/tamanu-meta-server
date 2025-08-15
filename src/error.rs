use std::{env::VarError, str::FromStr as _};

use axum::{
	http::StatusCode,
	response::{IntoResponse, Response},
};
use diesel_async::pooled_connection::PoolError;
use http::Uri;
use problem_details::ProblemDetails;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum AppError {
	#[error("{0}")]
	Custom(String),

	#[error("environment: {0}")]
	Environment(#[from] VarError),

	#[error("header: {0}")]
	Header(String),

	#[error("version parse error: {0}")]
	VersionParse(#[from] node_semver::SemverError),

	#[error("database: {0}")]
	DatabasePool(#[from] mobc::Error<PoolError>),

	#[error("database: {0}")]
	DatabaseQuery(#[from] diesel::result::Error),

	#[error("render: {0}")]
	Tera(#[from] tera::Error),

	#[error("io: {0}")]
	Io(String),

	#[error("no versions match given range")]
	NoMatchingVersions,

	#[error("version range is not usable")]
	UnusableRange,

	#[error("timesync: {0}")]
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

impl IntoResponse for AppError {
	fn into_response(self) -> Response {
		let status = match self {
			Self::NoMatchingVersions => StatusCode::NOT_FOUND,
			Self::UnusableRange => StatusCode::BAD_REQUEST,
			_ => StatusCode::INTERNAL_SERVER_ERROR,
		};

		let problem = ProblemDetails::new()
			.with_status(status)
			.with_title(self.to_string())
			.with_detail(format!("{self:?}"))
			.with_type(
				Uri::from_str(&format!(
					"/errors/{slug}",
					slug = match self {
						Self::Custom(_) => "other",
						Self::Environment(_) => "environment",
						Self::Header(_) => "header",
						Self::VersionParse(_) => "version-parse",
						Self::DatabasePool(_) => "database",
						Self::DatabaseQuery(_) => "database",
						Self::Tera(_) => "render",
						Self::Io(_) => "io",
						Self::NoMatchingVersions => "no-matching-versions",
						Self::UnusableRange => "unusable-range",
						Self::Timesync(_) => "timesync",
					}
				))
				.unwrap(),
			);

		let mut res = problem.into_response();
		*res.status_mut() = status;
		res
	}
}
