use rocket::{
	http::{Header, Status},
	request::{FromRequest, Outcome},
	Request,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ServerType;

impl From<ServerType> for Header<'_> {
	fn from(_: ServerType) -> Self {
		Header::new("X-Tamanu-Server", "Tamanu Metadata Server")
	}
}

#[derive(Debug, Clone)]
pub struct ServerTypeHeader(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ServerTypeHeader {
	type Error = ();

	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		match request.headers().get_one("X-Tamanu-Server") {
			Some(value) => Outcome::Success(ServerTypeHeader(value.to_string())),
			None => Outcome::Forward(Status::BadRequest),
		}
	}
}
