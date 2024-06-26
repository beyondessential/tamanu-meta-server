use std::time::Instant;

use chrono::{DateTime, Utc};
use futures::stream::{FuturesOrdered, StreamExt};
use rocket::serde::Serialize;
use rocket_db_pools::diesel::prelude::*;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use uuid::Uuid;

use crate::{
	launch::{Db, TamanuHeaders, Version},
	servers::{get_servers, Server},
};

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::statuses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Status {
	pub id: Uuid,
	pub created_at: DateTime<Utc>,
	pub server_id: Uuid,
	pub latency_ms: Option<i32>,
	pub version: Option<Version>,
	pub error: Option<String>,
}

impl Status {
	pub fn is_success(&self) -> bool {
		self.error.is_none()
	}
}

async fn ping_server(server: &Server) -> Status {
	let start = Instant::now();
	let (version, error) = reqwest::get(server.host.0.join("/api/").unwrap())
		.await
		.map_err(|err| err.to_string())
		.and_then(|res| {
			res.headers()
				.get("X-Version")
				.ok_or_else(|| "X-Version header not present".to_string())
				.and_then(|value| value.to_str().map_err(|err| err.to_string()))
				.and_then(|value| {
					node_semver::Version::parse(value)
						.map(Version)
						.map_err(|err| err.to_string())
				})
		})
		.map_or_else(|error| (None, Some(error)), |version| (Some(version), None));

	Status {
		id: Uuid::new_v4(),
		server_id: server.id,
		created_at: Utc::now(),
		latency_ms: Some(start.elapsed().as_millis().try_into().unwrap_or(i32::MAX)),
		version,
		error,
	}
}

async fn ping_servers(db: &mut Connection<Db>) -> Vec<(Status, Server)> {
	let statuses = FuturesOrdered::from_iter(
		get_servers(db)
			.await
			.into_iter()
			.map(|server| async move { (ping_server(&server).await, server) }),
	);

	statuses.collect().await
}

#[get("/")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Template> {
	use crate::schema::statuses::dsl::*;

	let servers = ping_servers(&mut db).await;
	diesel::insert_into(statuses)
		.values(
			&servers
				.iter()
				.map(|(status, _)| status.clone())
				.collect::<Vec<_>>(),
		)
		.execute(&mut db)
		.await
		.expect("Error inserting statuses");

	TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			servers,
		},
	))
}
