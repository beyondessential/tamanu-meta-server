use std::{env::VarError, str::FromStr as _};

#[cfg(feature = "ssr")]
use axum::response::{IntoResponse, Response};
#[cfg(feature = "ssr")]
use diesel_async::pooled_connection::PoolError;
use http::{StatusCode, Uri};
use leptos::{
	prelude::{FromServerFnError, ServerFnErrorErr},
	server_fn::codec::JsonEncoding,
};
use problem_details::ProblemDetails;
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum AppError {
	#[error("{0}")]
	Custom(String),

	#[error("{0}")]
	Problem(Box<ProblemDetails>),

	#[error("not implemented")]
	NotImplemented,

	#[error("environment: {0}")]
	Environment(#[from] VarError),

	#[error("header: {0}")]
	Header(String),

	#[error("version parse error: {0}")]
	VersionParse(Box<node_semver::SemverError>),

	#[cfg(feature = "ssr")]
	#[error("database: {0}")]
	DatabasePool(#[from] mobc::Error<PoolError>),

	#[cfg(feature = "ssr")]
	#[error("database: {0}")]
	DatabaseQuery(#[from] diesel::result::Error),

	#[cfg(feature = "ssr")]
	#[error("render: {0}")]
	Tera(#[from] tera::Error),

	#[error("io: {0}")]
	Io(String),

	#[error("no versions match given range")]
	NoMatchingVersions,

	#[error("version range is not usable")]
	UnusableRange,

	#[cfg(feature = "ssr")]
	#[error("timesync: {0}")]
	Timesync(#[from] timesimp::ParseError),

	#[error("missing authentication header: {0}")]
	AuthMissingHeader(&'static str),

	#[error("missing authentication certificate")]
	AuthMissingCertificate,

	#[error("invalid certificate format")]
	AuthInvalidCertificate(String),

	#[error("certificate not found or inactive")]
	AuthCertificateNotFound,

	#[error("insufficient permissions: {required} role required")]
	AuthInsufficientPermissions { required: String },

	#[error("authentication failed: {reason}")]
	AuthFailed { reason: String },

	#[error("server error: {0}")]
	ServerFn(#[from] ServerFnErrorErr),
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

impl From<ProblemDetails> for AppError {
	fn from(err: ProblemDetails) -> Self {
		Self::Problem(Box::new(err))
	}
}

impl From<node_semver::SemverError> for AppError {
	fn from(err: node_semver::SemverError) -> Self {
		Self::VersionParse(Box::new(err))
	}
}

impl FromServerFnError for AppError {
	type Encoder = JsonEncoding;
	fn from_server_fn_error(value: ServerFnErrorErr) -> Self {
		AppError::ServerFn(value)
	}
}

#[cfg(feature = "ssr")]
impl IntoResponse for AppError {
	fn into_response(self) -> Response {
		let status = self.to_http_status();
		let problem = self.to_problem_details();
		let mut res = problem.into_response();
		*res.status_mut() = status;
		res
	}
}

impl Clone for AppError {
	fn clone(&self) -> Self {
		Self::Problem(Box::new(self.to_problem_details()))
	}
}

impl AppError {
	fn to_http_status(&self) -> StatusCode {
		match self {
			Self::NotImplemented => StatusCode::NOT_IMPLEMENTED,
			Self::NoMatchingVersions => StatusCode::NOT_FOUND,
			Self::UnusableRange => StatusCode::BAD_REQUEST,
			#[cfg(feature = "ssr")]
			Self::DatabaseQuery(diesel::result::Error::NotFound) => StatusCode::NOT_FOUND,
			Self::AuthMissingHeader(_) => StatusCode::UNAUTHORIZED,
			Self::AuthMissingCertificate => StatusCode::UNAUTHORIZED,
			Self::AuthInvalidCertificate(_) => StatusCode::BAD_REQUEST,
			Self::AuthCertificateNotFound => StatusCode::UNAUTHORIZED,
			Self::AuthInsufficientPermissions { .. } => StatusCode::FORBIDDEN,
			Self::AuthFailed { .. } => StatusCode::UNAUTHORIZED,
			_ => StatusCode::INTERNAL_SERVER_ERROR,
		}
	}

	fn to_problem_details(&self) -> ProblemDetails {
		if let Self::Problem(problem) = self {
			return *problem.clone();
		}

		let status = self.to_http_status();
		ProblemDetails::new()
			.with_status(status)
			.with_title(self.to_string())
			.with_detail(format!("{self:?}"))
			.with_type(
				Uri::from_str(&format!(
					"/errors/{slug}",
					slug = match self {
						Self::Custom(_) => "other",
						Self::NotImplemented => "not-implemented",
						Self::Environment(_) => "environment",
						Self::Header(_) => "header",
						Self::VersionParse(_) => "version-parse",
						#[cfg(feature = "ssr")]
						Self::DatabasePool(_) => "database",
						#[cfg(feature = "ssr")]
						Self::DatabaseQuery(diesel::result::Error::NotFound) => "resource-not-found",
						#[cfg(feature = "ssr")]
						Self::DatabaseQuery(_) => "database",
						#[cfg(feature = "ssr")]
						Self::Tera(_) => "render",
						Self::Io(_) => "io",
						Self::NoMatchingVersions => "no-matching-versions",
						Self::UnusableRange => "unusable-range",
						#[cfg(feature = "ssr")]
						Self::Timesync(_) => "timesync",
						Self::AuthMissingHeader(_) => "auth-missing-header",
						Self::AuthMissingCertificate => "auth-missing-certificate",
						Self::AuthInvalidCertificate(_) => "auth-invalid-certificate",
						Self::AuthCertificateNotFound => "auth-certificate-not-found",
						Self::AuthInsufficientPermissions { .. } => "auth-insufficient-permissions",
						Self::AuthFailed { .. } => "auth-failed",
						Self::ServerFn(_) => "server-fn",
						Self::Problem(_) => unreachable!(),
					}
				))
				.unwrap(),
			)
	}
}

impl Serialize for AppError {
	fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		self.to_problem_details().serialize(serializer)
	}
}

impl<'de> Deserialize<'de> for AppError {
	fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let value = ProblemDetails::deserialize(deserializer)?;
		Ok(AppError::Problem(Box::new(value)))
	}
}
