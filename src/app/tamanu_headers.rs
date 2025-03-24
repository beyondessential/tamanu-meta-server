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
			server_type: ServerType::Meta,
			version: Version(node_semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap()),
		}
	}
}

#[derive(Debug, Clone)]
pub enum ServerTypeHeader {
	Central,
	Facility,
}

impl From<ServerTypeHeader> for Option<ServerType> {
	fn from(val: ServerTypeHeader) -> Self {
		match val {
			ServerTypeHeader::Central => Some(ServerType::Central),
			ServerTypeHeader::Facility => Some(ServerType::Facility),
		}
	}
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ServerTypeHeader {
	type Error = ();

	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		match request.headers().get_one("X-Tamanu-Server") {
			Some(value) => match value {
				"Tamanu Sync Server" => Outcome::Success(ServerTypeHeader::Central),
				"Tamanu LAN Server" => Outcome::Success(ServerTypeHeader::Facility),
				_ => Outcome::Error((Status::BadRequest, ())),
			},
			None => Outcome::Forward(Status::BadRequest),
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
			Some(value) => {
				// Parse the string into your Version type
				match Version::from_param(value) {
					Ok(version) => Outcome::Success(VersionHeader(version)),
					Err(_) => Outcome::Forward(Status::BadRequest),
				}
			}
			None => Outcome::Forward(Status::BadRequest),
		}
	}
}
