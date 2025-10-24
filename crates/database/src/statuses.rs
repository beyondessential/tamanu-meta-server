use std::{
	str::FromStr as _,
	time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use commons_errors::{AppError, Result};
use commons_versions::VersionStr;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use futures::stream::{FuturesOrdered, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::servers::Server;

#[derive(
	Debug,
	Clone,
	Serialize,
	Deserialize,
	Queryable,
	Selectable,
	Insertable,
	Associations,
	QueryableByName,
)]
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

impl NewStatus {
	pub async fn save(self, db: &mut AsyncPgConnection) -> Result<Status> {
		diesel::insert_into(crate::schema::statuses::table)
			.values(self)
			.returning(Status::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
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
			.flatten()
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

	pub async fn latest_for_server(
		db: &mut AsyncPgConnection,
		server: Uuid,
	) -> Result<Option<Status>> {
		use crate::schema::statuses::dsl::*;

		statuses
			.select(Status::as_select())
			.filter(
				server_id
					.eq(server)
					.and(created_at.ge(diesel::dsl::sql("NOW() - INTERVAL '7 days'")))
					.and(id.ne(Uuid::nil())),
			)
			.order(created_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	pub async fn latest_for_servers(
		db: &mut AsyncPgConnection,
		server_ids: &[Uuid],
	) -> Result<Vec<Status>> {
		if server_ids.is_empty() {
			return Ok(Vec::new());
		}

		// Get the latest status for each server using DISTINCT ON
		let query = diesel::sql_query(
			"SELECT DISTINCT ON (server_id) id, created_at, server_id, device_id, version, extra
				FROM statuses
				WHERE server_id = ANY($1)
				AND created_at >= NOW() - INTERVAL '7 days'
				AND id != '00000000-0000-0000-0000-000000000000'
				ORDER BY server_id, created_at DESC",
		)
		.bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(server_ids);

		query.load::<Status>(db).await.map_err(AppError::from)
	}

	pub async fn production_versions(
		db: &mut AsyncPgConnection,
	) -> Result<Vec<commons_versions::VersionStr>> {
		use crate::schema::statuses::dsl as statuses_dsl;
		use crate::views::ordered_servers::dsl as servers_dsl;

		let production_server_ids: Vec<Uuid> = servers_dsl::ordered_servers
			.select(servers_dsl::id)
			.filter(servers_dsl::rank.eq(crate::server_rank::ServerRank::Production))
			.load(db)
			.await?;

		statuses_dsl::statuses
			.select((
				statuses_dsl::server_id,
				statuses_dsl::created_at,
				statuses_dsl::version,
			))
			.filter(
				statuses_dsl::server_id
					.eq_any(&production_server_ids)
					.and(statuses_dsl::created_at.ge(diesel::dsl::sql("NOW() - INTERVAL '7 days'")))
					.and(statuses_dsl::id.ne(Uuid::nil())),
			)
			.order((statuses_dsl::server_id, statuses_dsl::created_at.desc()))
			.distinct_on(statuses_dsl::server_id)
			.load::<(
				Uuid,
				chrono::DateTime<Utc>,
				Option<commons_versions::VersionStr>,
			)>(db)
			.await
			.map(|results| {
				results
					.into_iter()
					.filter_map(|(_, _, version)| version)
					.collect()
			})
			.map_err(AppError::from)
	}
}
