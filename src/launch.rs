use rocket::{http::Header, serde::Serialize};
use rocket_dyn_templates::Template;

#[derive(Debug, Responder)]
pub struct TamanuHeaders<T> {
	inner: T,
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

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct Version(pub node_semver::Version);

impl From<Version> for Header<'_> {
	fn from(version: Version) -> Self {
		Header::new("X-Tamanu-Version", version.0.to_string())
	}
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ServerType;

impl From<ServerType> for Header<'_> {
	fn from(_: ServerType) -> Self {
		Header::new("X-Tamanu-Server", "Tamanu Metadata Server")
	}
}

#[catch(404)]
fn not_found() -> TamanuHeaders<()> {
	TamanuHeaders::new(())
}

#[launch]
pub fn rocket() -> _ {
	rocket::build()
		.attach(Template::fairing())
		.register("/", catchers![not_found])
		.mount(
			"/",
			routes![
				crate::servers::list,
				crate::statuses::view,
				crate::versions::view
			],
		)
}
