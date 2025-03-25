use rocket::http::Header;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ServerType;

impl From<ServerType> for Header<'_> {
	fn from(_: ServerType) -> Self {
		Header::new("X-Tamanu-Client", "Tamanu Metadata Server")
	}
}
