#[macro_use]
extern crate rocket;

use std::time::Instant;

use futures::stream::{FuturesOrdered, StreamExt};
use rocket::http::Header;
use rocket::serde::{json::Json, Serialize};
use rocket_dyn_templates::{context, Template};
use url::Url;

#[derive(Debug, Responder)]
struct TamanuHeaders<T> {
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
struct Version(pub node_semver::Version);

impl From<Version> for Header<'_> {
	fn from(version: Version) -> Self {
		Header::new("X-Tamanu-Version", version.0.to_string())
	}
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct ServerType;

impl From<ServerType> for Header<'_> {
	fn from(_: ServerType) -> Self {
		Header::new("X-Tamanu-Server", "Tamanu Metadata Server")
	}
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
struct VersionResponse {
	version: node_semver::Version,
}

#[get("/version/<version>")]
fn version_view(version: &str) -> TamanuHeaders<Json<VersionResponse>> {
	todo!("resolve {version}")
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
enum ServerRank {
	Live,
	Demo,
	Dev,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
struct Server {
	name: String,
	#[serde(rename = "type")]
	rank: ServerRank,
	host: Url,
}

fn get_servers() -> Vec<Server> {
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
fn servers_list() -> TamanuHeaders<Json<Vec<Server>>> {
	TamanuHeaders::new(Json(get_servers()))
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
struct ServerStatus {
	#[serde(flatten)]
	server: Server,
	success: bool,
	latency: u128,
	version: Version,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
struct ServerError {
	#[serde(flatten)]
	server: Server,
	success: bool,
	error: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(untagged)]
enum ServerResult {
	Ok(ServerStatus),
	Err(ServerError),
}

impl From<Result<ServerStatus, ServerError>> for ServerResult {
	fn from(res: Result<ServerStatus, ServerError>) -> Self {
		match res {
			Ok(status) => Self::Ok(status),
			Err(error) => Self::Err(error),
		}
	}
}

async fn ping_servers() -> Vec<ServerResult> {
	let statuses = FuturesOrdered::from_iter(get_servers().into_iter().map(|server| async {
		let start = Instant::now();
		reqwest::get(server.host.join("/api/").unwrap())
			.await
			.map_err(|err| err.to_string())
			.and_then(|res| {
				let version = res
					.headers()
					.get("X-Version")
					.ok_or_else(|| "X-Version header not present".to_string())
					.and_then(|value| value.to_str().map_err(|err| err.to_string()))
					.and_then(|value| {
						node_semver::Version::parse(value).map_err(|err| err.to_string())
					})?;

				Ok(ServerStatus {
					server: server.clone(),
					success: true,
					latency: start.elapsed().as_millis(),
					version: Version(version),
				})
			})
			.map_err(|error| ServerError {
				server,
				success: false,
				error,
			})
			.into()
	}));

	statuses.collect().await
}

#[get("/")]
async fn statuses_view() -> TamanuHeaders<Template> {
	TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			statuses: ping_servers().await,
		},
	))
}

#[catch(404)]
fn not_found() -> TamanuHeaders<()> {
	TamanuHeaders::new(())
}

#[launch]
fn rocket() -> _ {
	rocket::build()
		.attach(Template::fairing())
		.register("/", catchers![not_found])
		.mount("/", routes![servers_list, statuses_view, version_view])
}
