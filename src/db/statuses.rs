use std::{
	error::Error,
	time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use futures::stream::{FuturesOrdered, StreamExt};
use ipnet::IpNet;
use rocket::serde::Serialize;
use rocket_db_pools::diesel::{AsyncPgConnection, prelude::*};
use uuid::Uuid;

use crate::{
	db::servers::Server,
	error::{AppError, Result},
	servers::version::Version,
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
	pub remote_ip: Option<IpNet>,
}

#[derive(Debug, Insertable)]
#[diesel(belongs_to(Server))]
#[diesel(table_name = crate::schema::statuses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewStatus {
	pub server_id: Uuid,
	pub latency_ms: Option<i32>,
	pub version: Option<Version>,
	pub error: Option<String>,
	pub remote_ip: Option<IpNet>,
	pub extra: serde_json::Value,
}

impl Default for NewStatus {
	fn default() -> Self {
		Self {
			server_id: Default::default(),
			latency_ms: Default::default(),
			version: Default::default(),
			error: Default::default(),
			remote_ip: Default::default(),
			extra: serde_json::Value::Object(Default::default()),
		}
	}
}

impl Status {
	pub fn is_success(&self) -> bool {
		self.error.is_none()
	}

	pub async fn ping_server(client: &reqwest::Client, server: &Server) -> Self {
		let start = Instant::now();
		let (version, error) = client
			.get(server.host.0.join("/api/public/ping").unwrap())
			.send()
			.await
			.map_err(|err| {
				err.source()
					.map_or_else(|| err.to_string(), |err| err.to_string())
			})
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

		Self {
			id: Uuid::new_v4(),
			server_id: server.id,
			created_at: Utc::now(),
			latency_ms: Some(start.elapsed().as_millis().try_into().unwrap_or(i32::MAX)),
			version,
			error,
			remote_ip: None,
		}
	}

	pub async fn ping_servers(db: &mut AsyncPgConnection) -> Result<Vec<(Self, Server)>> {
		let client = reqwest::ClientBuilder::new()
			.timeout(Duration::from_secs(10))
			.build()
			.unwrap();
		let statuses =
			FuturesOrdered::from_iter(Server::all_pingable(db).await?.into_iter().map({
				let client = client.clone();
				move |server| {
					let client = client.clone();
					async move { (Self::ping_server(&client, &server).await, server) }
				}
			}));

		Ok(statuses.collect().await)
	}

	pub async fn ping_servers_and_save(db: &mut AsyncPgConnection) -> Result<()> {
		use crate::schema::statuses::dsl::*;

		let servers = Self::ping_servers(db).await?;
		diesel::insert_into(statuses)
			.values(
				&servers
					.iter()
					.map(|(status, _)| status.clone())
					.collect::<Vec<_>>(),
			)
			.execute(db)
			.await
			.map_err(|err| AppError::Database(err.to_string()))?;

		Ok(())
	}
}
