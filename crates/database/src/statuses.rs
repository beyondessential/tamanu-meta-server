use std::{
	str::FromStr as _,
	time::{Duration, Instant},
};

use commons_errors::{AppError, Result};
use commons_types::{
	issue::Severity,
	server::rank::ServerRank,
	status::{CheckResult, HealthState, ShortStatus},
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

fn server_label(s: &Server) -> String {
	s.name
		.clone()
		.or_else(|| s.host.as_ref().map(|h| h.0.to_string()))
		.unwrap_or_else(|| s.id.to_string())
}

fn format_secs(secs: i64) -> String {
	let s = secs.max(0);
	if s >= 86_400 {
		format!("{}d", s / 86_400)
	} else if s >= 3_600 {
		format!("{}h", s / 3_600)
	} else if s >= 60 {
		format!("{}m", s / 60)
	} else {
		format!("{s}s")
	}
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
		// A server with no URL can't be reached externally — nothing to ping.
		let host = &server.host.as_ref()?.0;
		let start = Instant::now();
		let url = host.join("/api/public/ping").unwrap();
		debug!(%url, "pinging");
		match client.get(url).send().await.map(|res| {
			res.headers()
				.get("X-Version")
				.and_then(|value| value.to_str().ok())
				.and_then(|value| VersionStr::from_str(value).ok())
		}) {
			Ok(version) => {
				let latency = start.elapsed().as_millis().try_into().unwrap_or(i32::MAX);
				info!(server=%server.id, host=%host, %latency, "ping success");
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
				warn!(server=%server.id, host=%host, "ping failure: {err}");
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

	/// Sweep every server's most recent status. For each monitored server
	/// (`is_monitored = true`) whose freshness has crossed the per-server
	/// `alert_when_down_for` threshold, file (or close) a canopy-sourced
	/// issue keyed by [`REACHABILITY_REF`].
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
			.filter(|s| s.is_monitored && s.id != Uuid::nil())
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
		// `list_by_source_ref` is filtered by `server_ids`, so every row is
		// server-scoped (`server_id` is `Some`); drop any defensively.
		let issue_map: std::collections::HashMap<Uuid, &Issue> = existing_issues
			.iter()
			.filter_map(|i| i.server_id.map(|sid| (sid, i)))
			.collect();

		let now = Timestamp::now();
		let mut filed = 0usize;
		for server in &monitored {
			let threshold = server.alert_when_down_for.0;
			let elapsed = match status_map.get(&server.id) {
				Some(s) => now.duration_since(s.created_at).abs(),
				// No status ever recorded: treat as infinite downtime so the
				// threshold always trips. Caps at i64::MAX seconds for arithmetic.
				None => SignedDuration::MAX,
			};
			let down = elapsed >= threshold;
			let existing = issue_map.get(&server.id).copied();

			let event = match (down, existing) {
				(false, None) => continue,
				(false, Some(issue)) if !issue.active => continue,
				(false, Some(_)) => NewEvent {
					source: CANOPY_SOURCE.into(),
					r#ref: REACHABILITY_REF.into(),
					severity: Some(Severity::Info),
					description: None,
					message: format!("Server {} is reachable again", server_label(server)),
					active: Some(false),
					occurred_at: Some(now),
				},
				(true, _) => NewEvent {
					source: CANOPY_SOURCE.into(),
					r#ref: REACHABILITY_REF.into(),
					severity: Some(Severity::Error),
					description: None,
					message: format!(
						"Server {} has not reported for {} (threshold {})",
						server_label(server),
						format_secs(elapsed.as_secs()),
						format_secs(threshold.as_secs()),
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

	/// Most recent status row (across all servers) whose `health` array
	/// contains an entry for `check_name`. Used by the rule-editor UI to
	/// surface a realistic sample of the variables an operator can
	/// predicate on (the check's extras, the status-level extras, and
	/// the server's tags resolved up the group).
	pub async fn latest_for_check_name(
		db: &mut AsyncPgConnection,
		check_name: &str,
	) -> Result<Option<Status>> {
		use diesel::sql_types::{Text, Uuid as DUuid};
		use diesel::{QueryableByName, sql_query};

		#[derive(QueryableByName)]
		struct Picked {
			#[diesel(sql_type = DUuid, column_name = "id")]
			row_id: Uuid,
		}

		// Two-step: pick the id via raw SQL (JSONB containment needs a
		// parameterised JSON literal that Diesel's typed DSL doesn't
		// express cleanly), then load the typed Status row by id.
		let picked: Option<Picked> = sql_query(
			"SELECT id FROM statuses \
			 WHERE health @> jsonb_build_array(jsonb_build_object('check', $1::text)) \
			 ORDER BY created_at DESC LIMIT 1",
		)
		.bind::<Text, _>(check_name)
		.get_result(db)
		.await
		.optional()
		.map_err(AppError::from)?;
		let Some(Picked { row_id }) = picked else {
			return Ok(None);
		};

		use crate::schema::statuses::dsl;
		dsl::statuses
			.select(Self::as_select())
			.filter(dsl::id.eq(row_id))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
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

	/// The most recent status for `server` that carries a version — **not**
	/// bounded by the live 7-day window. Used for the status card's headline
	/// version, which should reflect the last version a server ever reported
	/// even if it's currently down (and hence has no recent status).
	pub async fn last_with_version_for_server(
		db: &mut AsyncPgConnection,
		server: Uuid,
	) -> Result<Option<Status>> {
		use crate::schema::statuses::dsl::*;

		statuses
			.select(Status::as_select())
			.filter(
				server_id
					.eq(server)
					.and(version.is_not_null())
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

		// One LATERAL probe per server rather than DISTINCT ON over the
		// window: DISTINCT ON can't terminate early, so it reads every
		// status row in the 7-day window for each server. The LIMIT 1 under
		// the lateral join reads one row per weekly partition through the
		// (server_id, created_at DESC) composite index.
		let query = diesel::sql_query(
			"SELECT st.* FROM unnest($1) AS s(id) \
			 CROSS JOIN LATERAL ( \
				SELECT * FROM statuses \
				WHERE server_id = s.id \
				AND created_at >= NOW() - INTERVAL '7 days' \
				AND id != '00000000-0000-0000-0000-000000000000' \
				ORDER BY created_at DESC LIMIT 1 \
			 ) st",
		)
		.bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(server_ids);

		query.load::<Status>(db).await.map_err(AppError::from)
	}

	pub async fn production_versions(db: &mut AsyncPgConnection) -> Result<Vec<VersionStr>> {
		use crate::schema::servers::dsl as servers_dsl;

		let production_server_ids: Vec<Uuid> = servers_dsl::servers
			.select(servers_dsl::id)
			.filter(servers_dsl::rank.eq(ServerRank::Production))
			.load(db)
			.await?;

		Ok(Self::latest_for_servers(db, &production_server_ids)
			.await?
			.into_iter()
			.filter_map(|s| s.version)
			.collect())
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
	/// row's per-check results. The top-level `healthy` bool is being
	/// retired from the wire (absent ⇒ true on ingestion), so this
	/// rollup is primarily result-driven, consulting top-level only
	/// as legacy input:
	///
	/// - top-level `false` ⇒ [`HealthState::Unhealthy`] (legacy
	///   self-report; new bestool doesn't send it)
	/// - any entry with an explicit `result: failed` ⇒ `Unhealthy` —
	///   this is exactly what legacy bestool folded into top-level
	///   `healthy: false`
	/// - any warning/broken entry, or a legacy `healthy: false` entry
	///   under top-level `true` (legacy bestool's warning encoding) ⇒
	///   [`HealthState::Warning`]
	/// - otherwise [`HealthState::Healthy`] (passed and skipped
	///   entries don't count against the server)
	pub fn health_state(&self) -> HealthState {
		if !self.healthy {
			return HealthState::Unhealthy;
		}
		let mut state = HealthState::Healthy;
		if let Some(arr) = self.health.as_array() {
			for entry in arr {
				let Some(obj) = entry.as_object() else {
					continue;
				};
				let Some(result) = CheckResult::from_entry(obj) else {
					continue;
				};
				match result {
					CheckResult::Failed if obj.contains_key("result") => {
						return HealthState::Unhealthy;
					}
					CheckResult::Failed | CheckResult::Warning | CheckResult::Broken => {
						state = HealthState::Warning;
					}
					CheckResult::Passed | CheckResult::Skipped => {}
				}
			}
		}
		state
	}

	/// Identified operators connected to the server as of this status
	/// row, from the `external_users` check. Display fields are unfilled;
	/// see [`commons_types::status::operators_from_health`].
	pub fn operators(&self) -> Vec<commons_types::status::OperatorPresence> {
		commons_types::status::operators_from_health(&self.health)
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
		let current = self.version.as_ref().map(|v| &v.0)?;
		Some(version_distance(current, version))
	}
}

/// How far `current` lags behind `latest`, as `major_distance * 1000 +
/// minor_distance` with saturating subtraction (a newer-than-latest `current`
/// yields 0). Used for the status snapshot and the group card's headline.
pub fn version_distance(current: &Version, latest: &Version) -> u64 {
	let minor_distance = latest.minor.saturating_sub(current.minor);
	let major_distance = latest.major.saturating_sub(current.major);
	major_distance * 1000 + minor_distance
}
