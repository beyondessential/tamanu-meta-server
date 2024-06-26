use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use futures::stream::{FuturesOrdered, StreamExt};
use rocket::serde::Serialize;
use rocket_db_pools::{
	diesel::{prelude::*, AsyncPgConnection},
	Connection,
};
use rocket_dyn_templates::{context, Template};
use uuid::Uuid;

use crate::helper_types::pg_duration::PgDuration;
use crate::servers::{ServerRank, UrlField};
use crate::{
	launch::{Db, TamanuHeaders, Version},
	servers::{get_servers, Server},
};

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Insertable, Associations)]
#[diesel(belongs_to(Server))]
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

async fn ping_server(client: &reqwest::Client, server: &Server) -> Status {
	let start = Instant::now();
	let (version, error) = client
		.get(server.host.0.join("/api/").unwrap())
		.send()
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

async fn ping_servers(db: &mut AsyncPgConnection) -> Vec<(Status, Server)> {
	let client = reqwest::ClientBuilder::new()
		.timeout(Duration::from_secs(10))
		.build()
		.unwrap();
	let statuses = FuturesOrdered::from_iter(get_servers(db).await.into_iter().map({
		let client = client.clone();
		move |server| {
			let client = client.clone();
			async move { (ping_server(&client, &server).await, server) }
		}
	}));

	statuses.collect().await
}

pub async fn ping_servers_and_save(db: &mut AsyncPgConnection) {
	use crate::schema::statuses::dsl::*;

	let servers = ping_servers(db).await;
	diesel::insert_into(statuses)
		.values(
			&servers
				.iter()
				.map(|(status, _)| status.clone())
				.collect::<Vec<_>>(),
		)
		.execute(db)
		.await
		.expect("Error inserting statuses");
}

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::views::latest_statuses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct LatestStatus {
	pub server_id: Uuid,
	pub server_created_at: DateTime<Utc>,
	pub server_updated_at: DateTime<Utc>,
	pub server_name: String,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub server_rank: ServerRank,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub server_host: UrlField,

	pub is_up: bool,
	pub latest_latency: Option<i32>,

	pub latest_success_id: Option<Uuid>,
	pub latest_success_ts: Option<DateTime<Utc>>,
	pub latest_success_ago: Option<PgDuration>,
	pub latest_success_version: Option<Version>,

	pub latest_error_id: Option<Uuid>,
	pub latest_error_ts: Option<DateTime<Utc>>,
	pub latest_error_ago: Option<PgDuration>,
	pub latest_error_message: Option<String>,
}

pub async fn fetch_latest_statuses(db: &mut AsyncPgConnection) -> Vec<LatestStatus> {
	use crate::views::latest_statuses::dsl::*;

	latest_statuses
		.select(LatestStatus::as_select())
		.load(db)
		.await
		.expect("Error loading statuses")
}

#[get("/")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Template> {
	let entries = fetch_latest_statuses(&mut db).await;
	TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			entries,
		},
	))
}
