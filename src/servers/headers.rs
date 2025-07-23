use super::{server_type::ServerType, version::Version};
use rocket::{
	http::Status,
	request::{FromParam, FromRequest, Outcome},
	Request,
};

#[derive(Debug, Responder)]
pub struct TamanuHeaders<T> {
	pub inner: T,
	version: Version,
	server_type: ServerType,
}

impl<T> TamanuHeaders<T> {
	pub fn new(inner: T) -> Self {
		Self {
			inner,
			server_type: ServerType,
			version: Version(node_semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap()),
		}
	}
}

#[derive(Debug, Clone)]
pub struct VersionHeader(pub Version);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for VersionHeader {
	type Error = ();

	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		match request.headers().get_one("X-Version") {
			Some(value) => match Version::from_param(value) {
				Ok(version) => Outcome::Success(VersionHeader(version)),
				Err(_) => Outcome::Forward(Status::BadRequest),
			},
			None => Outcome::Forward(Status::BadRequest),
		}
	}
}
