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

/// The client stream canopy's own views read: `bestool` is the agent whose
/// heartbeats carry the authoritative health checks and version. Other
/// clients' streams are stored and answered per client, but do not (yet)
/// feed the status board, version tracking, or health issues.
pub const DEFAULT_CLIENT: &str = "bestool";

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

/// A stored status report from a server: one heartbeat's worth of
/// self-reported health, version, and extra data.
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
	/// Unique identifier of this status record.
	pub id: Uuid,
	/// When this status was received.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// The server this status was reported for.
	pub server_id: Uuid,
	/// The device that submitted this status, or `null` for statuses
	/// generated internally (e.g. by the reachability sweep).
	pub device_id: Option<Uuid>,
	/// The software version the server reported, if it reported one.
	pub version: Option<VersionStr>,
	/// Free-form extra data from the report (uptime, database version,
	/// timezone, etc.), stored verbatim as a JSON object.
	pub extra: serde_json::Value,
	/// Server's overall self-reported health. A report that omits this is
	/// recorded as healthy.
	pub healthy: bool,
	/// Per-check breakdown, as an array of objects each with at least a
	/// `check` name and a result; any extra per-check fields are passed
	/// through verbatim.
	pub health: serde_json::Value,
	/// The reporting agent this status is attributed to; `bestool` when the
	/// report named none.
	pub client: String,
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
	pub client: String,
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
			client: "bestool".to_owned(),
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
					// Canopy-generated reachability status; attributed to the
					// default client stream.
					client: "bestool".to_owned(),
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
		// Any-client freshness: a server is reachable while any of its
		// clients is still reporting.
		let statuses = Self::last_report_for_servers(db, &server_ids).await?;
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
			// No status ever recorded ⇒ `None`, which always trips the
			// threshold below.
			let elapsed: Option<SignedDuration> = status_map
				.get(&server.id)
				.map(|s| now.duration_since(s.created_at).abs());
			let down = match elapsed {
				Some(e) => e >= threshold,
				None => true,
			};
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
					message: match elapsed {
						Some(e) => format!(
							"Server {} has not reported for {} (threshold {})",
							server_label(server),
							format_secs(e.as_secs()),
							format_secs(threshold.as_secs()),
						),
						None => format!(
							"Server {} has never reported (threshold {})",
							server_label(server),
							format_secs(threshold.as_secs()),
						),
					},
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
					.and(client.eq(DEFAULT_CLIENT))
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
					.and(client.eq(DEFAULT_CLIENT))
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
					.and(client.eq(DEFAULT_CLIENT))
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
				AND client = $2 \
				AND created_at >= NOW() - INTERVAL '7 days' \
				AND id != '00000000-0000-0000-0000-000000000000' \
				ORDER BY created_at DESC LIMIT 1 \
			 ) st",
		)
		.bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(server_ids)
		.bind::<diesel::sql_types::Text, _>(DEFAULT_CLIENT);

		query.load::<Status>(db).await.map_err(AppError::from)
	}

	/// Most recent report per server from **any** client, for freshness
	/// decisions only: a server is reporting while any of its clients is,
	/// so one agent going quiet does not by itself make the server look
	/// down. The returned row belongs to whichever client reported last —
	/// read its timestamp, not its content.
	pub async fn last_report_for_servers(
		db: &mut AsyncPgConnection,
		server_ids: &[Uuid],
	) -> Result<Vec<Status>> {
		if server_ids.is_empty() {
			return Ok(Vec::new());
		}

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

	/// Live (non-deleted, non-canopy-own) servers whose latest status
	/// reports `check_name` at **any** result, paired with that status —
	/// the data backing the per-healthcheck "who's affected" page (which
	/// shows the failing servers by default and the healthy ones behind a
	/// toggle). Mirrors the live-server scoping in
	/// [`crate::servers::Server::get_all`] / the status-board queries:
	/// archived servers and canopy's own row (`id = Uuid::nil()`) never
	/// appear.
	///
	/// The check-name filter stays on the Rust side rather than folding
	/// into the SQL: reproducing [`CheckResult::from_entry`]'s legacy
	/// `healthy: bool` fallback as a jsonb predicate would just duplicate
	/// logic that already lives in [`Self::check_entry`], and the
	/// per-server latest-status set is small.
	pub async fn reporting_check_with_servers(
		db: &mut AsyncPgConnection,
		check_name: &str,
	) -> Result<Vec<(Server, Status)>> {
		use crate::schema::servers::dsl as servers_dsl;

		let servers: Vec<Server> = servers_dsl::servers
			.select(Server::as_select())
			.filter(servers_dsl::id.ne(Uuid::nil()))
			.filter(servers_dsl::deleted_at.is_null())
			.load(db)
			.await
			.map_err(AppError::from)?;
		let server_ids: Vec<Uuid> = servers.iter().map(|s| s.id).collect();

		let mut status_map: std::collections::HashMap<Uuid, Status> =
			Self::latest_for_servers(db, &server_ids)
				.await?
				.into_iter()
				.map(|s| (s.server_id, s))
				.collect();

		Ok(servers
			.into_iter()
			.filter_map(|s| {
				let status = status_map.remove(&s.id)?;
				status
					.check_entry(check_name)
					.is_some()
					.then_some((s, status))
			})
			.collect())
	}

	/// This status row's `health[]` entry for `check_name`: the normalised
	/// result plus the entry's full JSON object (including the reserved
	/// `check`/`healthy`/`result` keys, so callers can ship it to a UI
	/// that renders it the same way the server-detail checks table does).
	/// `None` when the row doesn't report that check, or the entry is
	/// malformed (no readable result — same rule as every other `health[]`
	/// reader).
	pub fn check_entry(
		&self,
		check_name: &str,
	) -> Option<(CheckResult, serde_json::Map<String, serde_json::Value>)> {
		let arr = self.health.as_array()?;
		arr.iter().find_map(|entry| {
			let obj = entry.as_object()?;
			if obj.get("check")?.as_str()? != check_name {
				return None;
			}
			let result = CheckResult::from_entry(obj)?;
			Some((result, obj.clone()))
		})
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

	/// Node.js runtime version the server reported in its status payload
	/// (`nodeVersion` extra), if present. Preferred over scraping the device
	/// connection's User-Agent (see [`crate::devices::DeviceConnection::nodejs_version`]),
	/// which only reflects whichever transport happened to set that header.
	pub fn node_version(&self) -> Option<String> {
		self.extra("nodeVersion")
			.and_then(|v| v.as_str())
			.map(ToOwned::to_owned)
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
		self.health_state_ignoring(&Default::default())
	}

	/// Like [`Self::health_state`], but entries whose check name is in
	/// `silenced_checks` are treated as skipped: an operator-silenced
	/// check keeps recording its results, they just don't count toward
	/// the server's health rollup. Callers get the set from
	/// [`crate::silenced_refs::silenced_health_checks_for_servers`].
	///
	/// The legacy top-level `healthy: false` short-circuit still wins:
	/// that flag predates per-check results, so a false can't be
	/// attributed to (and excused by) any particular silenced check.
	pub fn health_state_ignoring(
		&self,
		silenced_checks: &std::collections::BTreeSet<String>,
	) -> HealthState {
		if !self.healthy {
			return HealthState::Unhealthy;
		}
		let mut state = HealthState::Healthy;
		if let Some(arr) = self.health.as_array() {
			for entry in arr {
				let Some(obj) = entry.as_object() else {
					continue;
				};
				if let Some(check) = obj.get("check").and_then(|v| v.as_str())
					&& silenced_checks.contains(check)
				{
					continue;
				}
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
