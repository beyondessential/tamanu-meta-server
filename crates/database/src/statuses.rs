use std::{
	str::FromStr as _,
	time::{Duration, Instant},
};

use commons_errors::{AppError, Result};
use commons_types::{
	issue::Severity,
	server::rank::ServerRank,
	status::{HealthState, ShortStatus},
	version::VersionStr,
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use futures::stream::{FuturesOrdered, StreamExt};
use jiff::{SignedDuration, Timestamp};
use node_semver::Version;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::issues::{Issue, NewEvent};
use crate::servers::Server;

/// Source value canopy uses when it files reachability issues on behalf of a
/// server. Combined with [`REACHABILITY_REF`] to dedupe / find-or-create.
pub const CANOPY_SOURCE: &str = "canopy";

/// Ref value canopy uses for the one reachability issue per server. Stable so
/// the find-or-create in [`NewEvent::save`] coalesces every cycle into the
/// same issue row.
pub const REACHABILITY_REF: &str = "reachability";

/// Severity for canopy to file when a server's most recent status puts it in
/// the given short state. `None` for `Up` — a healthy server doesn't open an
/// issue.
///
/// The mapping escalates one step at a time below the incident floor
/// (`Notice` → `Warning`), then jumps to `Error` (which opens an incident),
/// then `Critical` for the long-gone state.
pub fn reachability_severity(short: ShortStatus) -> Option<Severity> {
	match short {
		ShortStatus::Up => None,
		ShortStatus::Blip => Some(Severity::Notice),
		ShortStatus::Away => Some(Severity::Warning),
		ShortStatus::Down => Some(Severity::Error),
		ShortStatus::Gone => Some(Severity::Critical),
	}
}

fn server_label(s: &Server) -> String {
	s.name.clone().unwrap_or_else(|| s.host.0.to_string())
}

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
	utoipa::ToSchema,
)]
#[diesel(belongs_to(Server))]
#[diesel(table_name = crate::schema::statuses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Status {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub server_id: Uuid,
	pub device_id: Option<Uuid>,
	pub version: Option<VersionStr>,
	pub extra: serde_json::Value,
	/// Server's overall self-reported health. Absent in the payload ⇒ true
	/// (legacy compat); see `docs/plans/status-snapshots-and-health.md`.
	pub healthy: bool,
	/// Per-check breakdown. Each entry is an object with at least
	/// `{check: string, healthy: bool, ...}`; extra fields are passed
	/// through verbatim.
	pub health: serde_json::Value,
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
	pub healthy: bool,
	pub health: serde_json::Value,
}

impl Default for NewStatus {
	fn default() -> Self {
		Self {
			server_id: Default::default(),
			device_id: Default::default(),
			version: Default::default(),
			extra: serde_json::Value::Object(Default::default()),
			healthy: true,
			health: serde_json::Value::Array(Default::default()),
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
					created_at: Timestamp::now(),
					version,
					extra: Default::default(),
					// Pingtask doesn't know the server's self-reported health;
					// it only knows the server is reachable. Default to healthy
					// to avoid false-positive unhealthy events from this path.
					healthy: true,
					health: serde_json::Value::Array(Default::default()),
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
			.map_err(|err| AppError::custom(format!("failed to build HTTP client: {err}")))?;
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
				servers
					.iter()
					.map(|(status, _)| status.clone())
					.collect::<Vec<_>>(),
			)
			.execute(db)
			.await
			.map_err(AppError::from)?;

		Ok(())
	}

	/// Sweep every server's most recent status. For each non-silenced server
	/// whose state has crossed (or just left) one of the reachability tiers
	/// (`Blip`/`Away`/`Down`/`Gone`), file (or close) a canopy-sourced issue
	/// keyed by [`REACHABILITY_REF`].
	///
	/// Most servers report by pushing their own status to the public-server
	/// (so their `device_id` is non-null); the pingtask only handles legacy
	/// foreign servers without a registered device. Both paths feed the same
	/// `statuses` table, so this sweep doesn't care which one is in play.
	///
	/// Returns the number of events filed in this pass.
	pub async fn sweep_reachability(db: &mut AsyncPgConnection) -> Result<usize> {
		let servers = Server::get_all(db, 0, None).await?;
		let monitored: Vec<&Server> = servers
			.iter()
			.filter(|s| s.alert_when_down && s.id != Uuid::nil())
			.collect();
		if monitored.is_empty() {
			return Ok(0);
		}

		let server_ids: Vec<Uuid> = monitored.iter().map(|s| s.id).collect();
		let statuses = Self::latest_for_servers(db, &server_ids).await?;
		let status_map: std::collections::HashMap<Uuid, Status> =
			statuses.into_iter().map(|s| (s.server_id, s)).collect();
		let existing_issues =
			Issue::list_by_source_ref(db, CANOPY_SOURCE, REACHABILITY_REF, &server_ids).await?;
		let issue_map: std::collections::HashMap<Uuid, &Issue> =
			existing_issues.iter().map(|i| (i.server_id, i)).collect();

		let now = Timestamp::now();
		let mut filed = 0usize;
		for server in &monitored {
			let short = status_map
				.get(&server.id)
				.map(Self::short_status)
				.unwrap_or_default();
			let severity = reachability_severity(short);
			let existing = issue_map.get(&server.id).copied();

			let event = match (severity, existing) {
				(None, None) => continue,
				(None, Some(issue)) if !issue.active => continue,
				(None, Some(_)) => NewEvent {
					source: CANOPY_SOURCE.into(),
					r#ref: REACHABILITY_REF.into(),
					severity: Some(Severity::Info),
					description: None,
					message: format!("Server {} is reachable again", server_label(server)),
					active: Some(false),
					occurred_at: Some(now),
				},
				(Some(sev), _) => NewEvent {
					source: CANOPY_SOURCE.into(),
					r#ref: REACHABILITY_REF.into(),
					severity: Some(sev),
					description: None,
					message: format!(
						"Server {} hasn't reported (state: {short})",
						server_label(server),
					),
					active: Some(true),
					occurred_at: Some(now),
				},
			};
			event.save(db, server.id, None).await?;
			filed += 1;
		}
		Ok(filed)
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

	/// Most recent status row for `server` with `created_at <= at`. No
	/// time-window cap — operators reviewing historical issues need
	/// the actual contemporary status, even if it's old.
	pub async fn at_time(
		db: &mut AsyncPgConnection,
		server: Uuid,
		at: Timestamp,
	) -> Result<Option<Status>> {
		use crate::schema::statuses::dsl::*;

		statuses
			.select(Status::as_select())
			.filter(
				server_id
					.eq(server)
					.and(created_at.le(jiff_diesel::Timestamp::from(at)))
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
			"SELECT DISTINCT ON (server_id) id, created_at, server_id, device_id, version, extra, healthy, health
				FROM statuses
				WHERE server_id = ANY($1)
				AND created_at >= NOW() - INTERVAL '7 days'
				AND id != '00000000-0000-0000-0000-000000000000'
				ORDER BY server_id, created_at DESC",
		)
		.bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(server_ids);

		query.load::<Status>(db).await.map_err(AppError::from)
	}

	pub async fn production_versions(db: &mut AsyncPgConnection) -> Result<Vec<VersionStr>> {
		use crate::schema::servers::dsl as servers_dsl;
		use crate::schema::statuses::dsl as statuses_dsl;

		let production_server_ids: Vec<Uuid> = servers_dsl::servers
			.select(servers_dsl::id)
			.filter(servers_dsl::rank.eq(ServerRank::Production))
			.load(db)
			.await?;

		statuses_dsl::statuses
			.select((statuses_dsl::version,))
			.filter(
				statuses_dsl::server_id
					.eq_any(&production_server_ids)
					.and(statuses_dsl::created_at.ge(diesel::dsl::sql("NOW() - INTERVAL '7 days'")))
					.and(statuses_dsl::id.ne(Uuid::nil())),
			)
			.order((statuses_dsl::server_id, statuses_dsl::created_at.desc()))
			.distinct_on(statuses_dsl::server_id)
			.load::<(Option<VersionStr>,)>(db)
			.await
			.map(|results| {
				results
					.into_iter()
					.filter_map(|(version,)| version)
					.collect()
			})
			.map_err(AppError::from)
	}

	pub fn platform(&self) -> Option<String> {
		self.extra("pgVersion")
			.and_then(|pg| pg.as_str())
			.map(|pg| {
				if pg.contains("Visual C++") || pg.contains("windows") {
					"Windows"
				} else {
					"Linux"
				}
				.into()
			})
	}

	pub fn postgres_version(&self) -> Option<String> {
		self.extra("pgVersion")
			.and_then(|pg| pg.as_str())
			.and_then(|pg| pg.split_ascii_whitespace().nth(1))
			.map(|vers| vers.trim_end_matches(',').into())
	}

	/// Server's self-reported health state derived from this status
	/// row. Returns [`HealthState::Unhealthy`] if top-level is
	/// `false`, [`HealthState::Warning`] if any `health[]` entry is
	/// failing while top-level is `true`, and [`HealthState::Healthy`]
	/// otherwise.
	pub fn health_state(&self) -> HealthState {
		if !self.healthy {
			return HealthState::Unhealthy;
		}
		let any_failing = self.health.as_array().is_some_and(|arr| {
			arr.iter().any(|e| {
				e.as_object()
					.and_then(|o| o.get("healthy"))
					.and_then(|v| v.as_bool())
					.is_some_and(|b| !b)
			})
		});
		if any_failing {
			HealthState::Warning
		} else {
			HealthState::Healthy
		}
	}

	pub fn short_status(&self) -> ShortStatus {
		let since = self.created_at.duration_since(Timestamp::now()).abs();
		if since > SignedDuration::from_mins(30) {
			ShortStatus::Down
		} else if since > SignedDuration::from_mins(10) {
			ShortStatus::Away
		} else if since > SignedDuration::from_mins(2) {
			ShortStatus::Blip
		} else {
			ShortStatus::Up
		}
	}

	pub fn distance_from_version(&self, version: &Version) -> Option<u64> {
		let Some(current) = &self.version.as_ref().map(|v| &v.0) else {
			return None;
		};

		let minor_distance = version.minor.saturating_sub(current.minor);
		let major_distance = version.major.saturating_sub(current.major);
		Some(major_distance * 1000 + minor_distance)
	}
}
