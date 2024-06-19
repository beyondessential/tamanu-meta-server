use std::time::Instant;

use futures::stream::{FuturesOrdered, StreamExt};
use rocket::serde::Serialize;
use rocket_dyn_templates::{context, Template};

use crate::{
	launch::{TamanuHeaders, Version},
	servers::{get_servers, Server},
};

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
pub async fn view() -> TamanuHeaders<Template> {
	TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			statuses: ping_servers().await,
		},
	))
}
