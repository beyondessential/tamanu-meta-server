use std::{
	str::FromStr as _,
	time::{Duration, Instant},
};

use commons_errors::{AppError, Result};
use commons_types::{
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

use crate::issues::Issue;
use crate::servers::Server;

/// Source value canopy uses when it files reachability issues on behalf of a
/// server. Combined with [`REACHABILITY_REF`] to dedupe / find-or-create.
pub const CANOPY_SOURCE: &str = "canopy";

/// Ref value canopy uses for the one reachability issue per server. Stable so
/// the find-or-create in [`NewEvent::save`] coalesces every cycle into the
/// same issue row.
pub const REACHABILITY_REF: &str = "reachability";

/// Ref prefix for the per-(server, source) staleness checks canopy files
/// when a reporting source stops reporting: `stale/<source>`, e.g.
/// `stale/alertd`. A contract with stored silences and policy rows.
pub const STALE_REF_PREFIX: &str = "stale/";

pub const REACHABILITY_DOC: &str = "## Description

Nothing is reaching canopy about this server: no source has reported and no ping has succeeded within the server's down threshold. This is the all-sources-stale signal — the server is presented as unreachable.

## Results

- **fail** — no status from any source within the threshold (or ever); recovers as soon as anything reports.

## Solve

Check whether the server itself is down, its network/VPN path to canopy, and whether its reporting agents are running. The per-source `stale/<source>` checks narrow down which reporter went quiet first.";

pub const STALE_DOC: &str = "## Description

A source that has been reporting on this server has gone quiet: its most recent report is older than the server's down threshold, while other paths may still reach canopy.

## Results

- **warn** — the source's last report crossed the threshold; recovers when it reports again.

## Solve

Check that the reporting agent for this source is running on the server and can reach canopy. If the source was deliberately decommissioned, silence this check for the server.";

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
	/// The source that pushed this status: the named reporter (e.g.
	/// `alertd`), or `canopy` for statuses generated internally.
	pub source: String,
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
	pub source: String,
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
			source: CANOPY_SOURCE.into(),
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
					source: CANOPY_SOURCE.into(),
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

	/// Sweep every monitored server (`is_monitored = true`) for staleness
	/// against its per-server `alert_when_down_for` threshold, on two
	/// levels:
	///
	/// - **Reachability**: the server's most recent status row, from any
	///   source. Every source's report (and every pingtask probe) lands a
	///   status row, so this goes stale exactly when *all* of the server's
	///   sources are stale — the server is unreachable. Filed (or closed)
	///   as a canopy-sourced check keyed by [`REACHABILITY_REF`]. This arm
	///   also covers servers that have never reported at all, and legacy
	///   foreign servers without a registered device that only the
	///   pingtask reaches.
	/// - **Per-source staleness**: a source that has reported checks on a
	///   server is expected to keep reporting. When its most recent report
	///   is older than the threshold, file a `stale/<source>` check (see
	///   [`STALE_REF_PREFIX`]) — catching one reporter going quiet while
	///   others keep the server reachable.
	///
	/// Returns the number of events filed in this pass.
	pub async fn sweep_staleness(db: &mut AsyncPgConnection) -> Result<usize> {
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

			let (observed, message) = match (down, existing) {
				(false, None) => continue,
				(false, Some(issue)) if !issue.active => continue,
				(false, Some(_)) => (
					CheckResult::Passed,
					format!("Server {} is reachable again", server_label(server)),
				),
				(true, _) => (
					CheckResult::Failed,
					match elapsed {
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
				),
			};
			crate::issues::file_check(
				db,
				crate::issues::CheckFiling {
					source: CANOPY_SOURCE,
					scope: crate::issues::FilingScope::Server {
						server_id: server.id,
						device_id: None,
					},
					check: REACHABILITY_REF,
					observed,
					title: Some("Server unreachable"),
					message: &message,
					detail: Some(serde_json::json!({
						"elapsed_secs": elapsed.map(|e| e.as_secs()),
						"threshold_secs": threshold.as_secs(),
					})),
					default_ceiling: CheckResult::Failed,
					default_escalates: false,
					documentation: Some(REACHABILITY_DOC),
				},
			)
			.await?;
			filed += 1;
		}

		filed += Self::sweep_source_staleness(db, &monitored, now).await?;
		Ok(filed)
	}

	/// The per-source arm of [`Self::sweep_staleness`]: file (or close) a
	/// `stale/<source>` check for each (monitored server, reporting source)
	/// whose most recent report has crossed the server's threshold.
	///
	/// Freshness comes from check state, not the statuses history: every
	/// report re-stamps `last_seen` on the state rows of the checks it
	/// mentions, so [`Issue::source_freshness`] is both the set of sources
	/// expected to report and when each last did. Registered at a warning
	/// ceiling — one quiet reporter degrades a server, full unreachability
	/// is the reachability arm's failure.
	async fn sweep_source_staleness(
		db: &mut AsyncPgConnection,
		monitored: &[&Server],
		now: Timestamp,
	) -> Result<usize> {
		let server_map: std::collections::HashMap<Uuid, &Server> =
			monitored.iter().map(|s| (s.id, *s)).collect();
		let server_ids: Vec<Uuid> = monitored.iter().map(|s| s.id).collect();
		let freshness = Issue::source_freshness(db, &server_ids).await?;
		if freshness.is_empty() {
			return Ok(0);
		}

		let refs: Vec<String> = freshness
			.iter()
			.map(|(_, source, _)| format!("{STALE_REF_PREFIX}{source}"))
			.collect::<std::collections::BTreeSet<_>>()
			.into_iter()
			.collect();
		let existing = Issue::list_by_source_refs(db, CANOPY_SOURCE, &refs, &server_ids).await?;
		let existing_map: std::collections::HashMap<(Uuid, &str), &Issue> = existing
			.iter()
			.filter_map(|i| i.server_id.map(|sid| ((sid, i.r#ref.as_str()), i)))
			.collect();

		let mut filed = 0usize;
		for (server_id, source, last_seen) in &freshness {
			let Some(server) = server_map.get(server_id) else {
				continue;
			};
			let threshold = server.alert_when_down_for.0;
			let elapsed = now.duration_since(*last_seen).abs();
			let stale = elapsed >= threshold;
			let check = format!("{STALE_REF_PREFIX}{source}");
			let existing = existing_map.get(&(*server_id, check.as_str())).copied();

			let (observed, message) = match (stale, existing) {
				(false, None) => continue,
				(false, Some(issue)) if !issue.active => continue,
				(false, Some(_)) => (
					CheckResult::Passed,
					format!(
						"Source {source} on server {} is reporting again",
						server_label(server),
					),
				),
				(true, _) => (
					CheckResult::Failed,
					format!(
						"Source {source} on server {} has not reported for {} (threshold {})",
						server_label(server),
						format_secs(elapsed.as_secs()),
						format_secs(threshold.as_secs()),
					),
				),
			};
			crate::issues::file_check(
				db,
				crate::issues::CheckFiling {
					source: CANOPY_SOURCE,
					scope: crate::issues::FilingScope::Server {
						server_id: *server_id,
						device_id: None,
					},
					check: &check,
					observed,
					title: Some("Source stopped reporting"),
					message: &message,
					detail: Some(serde_json::json!({
						"source": source,
						"last_reported": last_seen.to_string(),
						"elapsed_secs": elapsed.as_secs(),
						"threshold_secs": threshold.as_secs(),
					})),
					default_ceiling: CheckResult::Warning,
					default_escalates: false,
					documentation: Some(STALE_DOC),
				},
			)
			.await?;
			filed += 1;
		}
		Ok(filed)
	}

	/// Most recent status row (across all servers) pushed by `source`
	/// whose `health` array contains an entry for `check_name`. Used by
	/// the rule-editor UI to surface a realistic sample of the variables
	/// an operator can predicate on (the check's extras, the status-level
	/// extras, and the server's tags resolved up the group). Scoped to
	/// the check's own source — another source's same-named check may
	/// carry entirely different fields.
	pub async fn latest_for_check_name(
		db: &mut AsyncPgConnection,
		source: &str,
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
			 WHERE source = $1 \
			 AND health @> jsonb_build_array(jsonb_build_object('check', $2::text)) \
			 ORDER BY created_at DESC LIMIT 1",
		)
		.bind::<Text, _>(source)
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
