use super::{server_type::ServerType, version::Version};

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

	pub fn take_inner(self) -> T {
		self.inner
	}
}
