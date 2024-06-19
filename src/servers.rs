use rocket::serde::{json::Json, Serialize};
use url::Url;

use crate::launch::TamanuHeaders;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerRank {
	Live,
	Demo,
	Dev,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct Server {
	pub name: String,
	#[serde(rename = "type")]
	pub rank: ServerRank,
	pub host: Url,
}

pub fn get_servers() -> Vec<Server> {
	vec![
		Server {
			name: "Kiribati".into(),
			rank: ServerRank::Live,
			host: Url::parse("https://sync.tamanu-kiribati.org").unwrap(),
		},
		Server {
			name: "Demo 2".into(),
			rank: ServerRank::Demo,
			host: Url::parse("https://central-demo2.internal.tamanu.io").unwrap(),
		},
		Server {
			name: "RC (2.6)".into(),
			rank: ServerRank::Dev,
			host: Url::parse("https://central.release-2-6.internal.tamanu.io").unwrap(),
		},
	]
}

#[get("/servers")]
pub fn list() -> TamanuHeaders<Json<Vec<Server>>> {
	TamanuHeaders::new(Json(get_servers()))
}
