use rocket::serde::{json::Json, Serialize};

use crate::launch::TamanuHeaders;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct VersionResponse {
	version: node_semver::Version,
}

#[get("/version/<version>")]
pub fn view(version: &str) -> TamanuHeaders<Json<VersionResponse>> {
	todo!("resolve {version}")
}
