use std::{
	str::FromStr as _,
	time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use futures::stream::{FuturesOrdered, StreamExt};

use serde::Serialize;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
	db::servers::Server,
	error::{AppError, Result},
	servers::version::VersionStr,
};

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Insertable, Associations)]
#[diesel(belongs_to(Server))]
#[diesel(table_name = crate::schema::statuses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Status {
	pub id: Uuid,
	pub created_at: DateTime<Utc>,
	pub server_id: Uuid,
	pub device_id: Option<Uuid>,
	pub version: Option<VersionStr>,

	pub extra: serde_json::Value,
}

#[derive(Debug, Insertable)]
#[diesel(belongs_to(Server))]
#[diesel(table_name = crate::schema::statuses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewStatus {
	pub server_id: Uuid,
	pub device_id: Option<Uuid>,
	pub version: Option<VersionStr>,

	pub extra: serde_json::Value,
}

impl Default for NewStatus {
	fn default() -> Self {
		Self {
			server_id: Default::default(),
			device_id: Default::default(),
			version: Default::default(),
			extra: serde_json::Value::Object(Default::default()),
		}
	}
}

impl Status {
	pub fn extra(&self, key: &str) -> Option<&serde_json::Value> {
		self.extra.as_object().and_then(|obj| obj.get(key))
	}

	pub async fn ping_server(client: &reqwest::Client, server: &Server) -> Option<Self> {
		let start = Instant::now();
		let url = server.host.0.join("/api/public/ping").unwrap();
		debug!(%url, "pinging");
		match client.get(url).send().await.map(|res| {
			res.headers()
				.get("X-Version")
				.and_then(|value| value.to_str().ok())
				.and_then(|value| VersionStr::from_str(value).ok())
		}) {
			Ok(version) => {
				let latency = start.elapsed().as_millis().try_into().unwrap_or(i32::MAX);
				info!(server=%server.id, host=%server.host.0, %latency, "ping success");
				Some(Self {
					id: Uuid::new_v4(),
					server_id: server.id,
					device_id: None,
					created_at: Utc::now(),
					version,

					extra: Default::default(),
				})
			}
			Err(err) => {
				warn!(server=%server.id, host=%server.host.0, "ping failure: {err}");
				None
			}
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
					async move {
						Self::ping_server(&client, &server)
							.await
							.map(|ping| (ping, server))
					}
				}
			}));

		Ok(statuses
			.collect::<Vec<Option<_>>>()
			.await
			.into_iter()
			.filter_map(|o| o)
			.collect())
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
			.map_err(AppError::from)?;

		Ok(())
	}

	pub async fn latest_for_all_servers(db: &mut AsyncPgConnection) -> Result<Vec<Status>> {
		use crate::schema::statuses::dsl::*;

		statuses
			.filter(created_at.ge(diesel::dsl::sql("NOW() - INTERVAL '1 month'")))
			.distinct_on(server_id)
			.order((server_id, created_at.desc()))
			.select(Status::as_select())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn server(&self, db: &mut AsyncPgConnection) -> Result<crate::db::servers::Server> {
		use crate::views::ordered_servers::dsl::*;

		let row = ordered_servers
			.filter(id.eq(self.server_id))
			.select(crate::db::servers::Server::as_select())
			.first::<crate::db::servers::Server>(db)
			.await
			.map_err(AppError::from)?;

		Ok(row)
	}

	pub async fn device_connection(
		&self,
		db: &mut AsyncPgConnection,
	) -> Result<Option<crate::db::devices::DeviceConnection>> {
		let Some(dev_id) = self.device_id else {
			return Ok(None);
		};

		use crate::schema::device_connections::dsl as dc;

		let row = dc::device_connections
			.filter(
				dc::device_id
					.eq(dev_id)
					.and(dc::created_at.le(self.created_at)),
			)
			.order(dc::created_at.desc())
			.select(crate::db::devices::DeviceConnection::as_select())
			.first::<crate::db::devices::DeviceConnection>(db)
			.await
			.optional()
			.map_err(AppError::from)?;

		Ok(row)
	}
}
