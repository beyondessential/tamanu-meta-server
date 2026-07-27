use std::{
	str::FromStr as _,
	time::{Duration, Instant},
};

use commons_errors::{AppError, Result};
use commons_types::{
	status::{CheckResult, ShortStatus},
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

pub const REACHABILITY_DOC: &str = "## Description

Tracks whether the sources canopy expects to report on this server are actually reporting. A source going quiet degrades the server rather than silently dropping its checks, so a dead reporter is never mistaken for health.

## Results

- **warn** — a source in reachability mode `on` has gone quiet past the server's down threshold while others still report; the quiet sources are listed in the detail.
- **fail** — every expected source is stale, or the server has never reported: nothing is reaching canopy, and the server is unreachable.

## Solve

Check whether the server is down, its network/VPN path to canopy, and whether its reporting agents are running. A source that was deliberately retired should be set to `quiet` or `off` in the source list.";

/// How far back a "last value this server ever reported" read may look.
///
/// `statuses` is range-partitioned by week and prod already carries ~100
/// partitions with the better part of a million rows *per server*. A
/// predicate on `server_id` alone cannot be partition-pruned, so a query
/// with no lower bound on `created_at` degrades into a scan of every
/// partition — measured at 217s in production for a single row. Worse, the
/// pruning-free plan reads far more than `shared_buffers` holds, so it
/// evicts the buffer pool and drags every unrelated query down with it.
///
/// Every lookback here is therefore capped. The cost is that a server quiet
/// for longer than the window reads as "never reported"; that is preferable
/// to an unbounded scan, and the windows are generous next to how long a
/// server can be down before someone notices.
const GRACE_LOOKBACK_SQL: &str = "NOW() - INTERVAL '30 days'";

/// [`GRACE_LOOKBACK_SQL`] as a span, for bounding a lookback relative to a
/// caller-supplied point in time rather than to `NOW()`.
const GRACE_LOOKBACK: SignedDuration = SignedDuration::from_hours(24 * 30);

/// Lookback for the last version a server reported. Longer than
/// [`GRACE_LOOKBACK_SQL`] because a group's displayed version is cached from
/// its canonical member, and a member down for a month or two should not
/// blank out the group's version label. Still bounded — see
/// [`GRACE_LOOKBACK_SQL`] for why an unbounded read is not an option.
const VERSION_LOOKBACK_SQL: &str = "NOW() - INTERVAL '90 days'";

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

	/// File (or update) each monitored server's single `reachability` check
	/// from the freshness of the sources it is expected to report, against
	/// its per-server `alert_when_down_for` threshold. Passed when every
	/// expected source is fresh; warning when an `on`-mode source has gone
	/// quiet while others still report (the quiet sources are named in the
	/// detail); failed when every expected source is stale, or the server
	/// has never reported — unreachable. Each source's reachability mode
	/// (`on`/`quiet`/`off`) governs whether its silence warns, only counts
	/// toward unreachable, or is ignored. Servers with no counted source
	/// fall back to whether anything at all has reached canopy.
	///
	/// Returns the number of events filed in this pass.
	pub async fn sweep_staleness(db: &mut AsyncPgConnection) -> Result<usize> {
		use commons_types::source::ReachabilityMode;
		use std::collections::HashMap;

		let servers = Server::get_all(db, 0, None).await?;
		let monitored: Vec<&Server> = servers
			.iter()
			.filter(|s| s.is_monitored && s.id != Uuid::nil())
			.collect();
		if monitored.is_empty() {
			return Ok(0);
		}
		let server_ids: Vec<Uuid> = monitored.iter().map(|s| s.id).collect();

		// Per-source freshness (already excludes canopy/manual and
		// decommissioned checks), grouped by server, plus each source's
		// reachability and ingest modes.
		let freshness = Issue::source_freshness(db, &server_ids).await?;
		let modes = crate::source_policies::SourcePolicy::modes(db).await?;
		let ingest = crate::source_policies::SourcePolicy::ingest_modes(db).await?;
		let mut by_server: HashMap<Uuid, Vec<(String, Timestamp)>> = HashMap::new();
		for (sid, source, last_seen) in freshness {
			by_server.entry(sid).or_default().push((source, last_seen));
		}

		// Backstop for servers with no counted source (never reported, or
		// only reached by pingtask): the latest status row, any source.
		let statuses = Self::latest_for_servers(db, &server_ids).await?;
		let status_map: HashMap<Uuid, Status> =
			statuses.into_iter().map(|s| (s.server_id, s)).collect();

		let existing_issues =
			Issue::list_by_source_ref(db, CANOPY_SOURCE, REACHABILITY_REF, &server_ids).await?;
		let issue_map: HashMap<Uuid, &Issue> = existing_issues
			.iter()
			.filter_map(|i| i.server_id.map(|sid| (sid, i)))
			.collect();

		let now = Timestamp::now();
		let mut filed = 0usize;
		for server in &monitored {
			let threshold = server.alert_when_down_for.0;
			let label = server_label(server);

			// Sources reporting on this server that count for reachability:
			// not switched off, and actually ingested (an ignored/denied
			// source has no fresh data to judge). With how long each has
			// been silent.
			let expected: Vec<(&str, SignedDuration, ReachabilityMode)> = by_server
				.get(&server.id)
				.into_iter()
				.flatten()
				.map(|(source, last_seen)| {
					let mode = modes.get(source).copied().unwrap_or_default();
					(source.as_str(), now.duration_since(*last_seen).abs(), mode)
				})
				.filter(|(source, _, mode)| {
					*mode != ReachabilityMode::Off
						&& ingest.get(*source).copied().unwrap_or_default()
							== commons_types::source::IngestMode::Allow
				})
				.collect();

			let (observed, message, detail) = if expected.is_empty() {
				let elapsed = status_map
					.get(&server.id)
					.map(|s| now.duration_since(s.created_at).abs());
				let down = elapsed.map(|e| e >= threshold).unwrap_or(true);
				if down {
					let message = match elapsed {
						Some(e) => format!(
							"Server {label} has not reported for {} (threshold {})",
							format_secs(e.as_secs()),
							format_secs(threshold.as_secs()),
						),
						None => format!(
							"Server {label} has never reported (threshold {})",
							format_secs(threshold.as_secs()),
						),
					};
					(
						CheckResult::Failed,
						message,
						serde_json::json!({
							"elapsed_secs": elapsed.map(|e| e.as_secs()),
							"threshold_secs": threshold.as_secs(),
						}),
					)
				} else {
					(
						CheckResult::Passed,
						format!("Server {label} is reachable"),
						serde_json::json!({ "threshold_secs": threshold.as_secs() }),
					)
				}
			} else {
				let stale: Vec<&(&str, SignedDuration, ReachabilityMode)> = expected
					.iter()
					.filter(|(_, e, _)| *e >= threshold)
					.collect();
				let stale_names = stale
					.iter()
					.map(|(s, _, _)| *s)
					.collect::<Vec<_>>()
					.join(", ");
				let stale_detail = stale
					.iter()
					.map(
						|(source, e, _)| serde_json::json!({ "source": source, "stale_secs": e.as_secs() }),
					)
					.collect::<Vec<_>>();
				let detail = serde_json::json!({
					"stale_sources": stale_detail,
					"threshold_secs": threshold.as_secs(),
				});
				if stale.len() == expected.len() {
					(
						CheckResult::Failed,
						format!(
							"Server {label} is unreachable: every source is stale ({stale_names})"
						),
						detail,
					)
				} else if stale
					.iter()
					.any(|(_, _, mode)| *mode == ReachabilityMode::On)
				{
					(
						CheckResult::Warning,
						format!("Source(s) on server {label} have gone quiet: {stale_names}"),
						detail,
					)
				} else {
					// Some stale, but every stale source is quiet: no warning.
					(
						CheckResult::Passed,
						format!("Server {label} is reachable"),
						serde_json::json!({ "threshold_secs": threshold.as_secs() }),
					)
				}
			};

			// Don't churn a passing reachability when there's nothing open to
			// close.
			if observed == CheckResult::Passed {
				match issue_map.get(&server.id) {
					None => continue,
					Some(issue) if !issue.active => continue,
					Some(_) => {}
				}
			}

			crate::issues::file_check(
				db,
				crate::issues::CheckFiling {
					source: CANOPY_SOURCE,
					scope: crate::issues::Scope::Server(server.id),
					device_id: None,
					check: REACHABILITY_REF,
					observed,
					title: Some("Server reachability"),
					message: &message,
					detail: Some(detail),
					default_ceiling: CheckResult::Failed,
					default_escalates: false,
					documentation: Some(REACHABILITY_DOC),
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
	///
	/// Bounded to [`GRACE_LOOKBACK_SQL`]: a recent sample is the useful one,
	/// and the `health @>` containment is not indexable, so without the
	/// window a check that has stopped reporting scans every partition
	/// across every server.
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
		let picked: Option<Picked> = sql_query(format!(
			"SELECT id FROM statuses \
			 WHERE source = $1 \
			 AND health @> jsonb_build_array(jsonb_build_object('check', $2::text)) \
			 AND created_at >= {GRACE_LOOKBACK_SQL} \
			 ORDER BY created_at DESC LIMIT 1"
		))
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

	/// Each source's most recent status for `server` at or before `at`
	/// (latest overall when `at` is `None`) — one row per source. Unlike
	/// [`Self::at_time`], which collapses to a single row regardless of
	/// source, this keeps every source's contribution, for reconstructing
	/// the consolidated multi-source checks view as of a point in time.
	///
	/// Bounded to [`GRACE_LOOKBACK`] before the cutoff, so a source silent
	/// for the whole window contributes nothing as of `at`. The bound is
	/// load-bearing: `DISTINCT ON` cannot terminate early (it has to see
	/// every candidate row before it knows which is each group's newest), so
	/// an upper bound alone made this read a server's entire status history —
	/// ~864k rows across every partition in prod — to return one row per
	/// source. This is the same trap [`Self::latest_for_servers`] documents.
	pub async fn latest_per_source_at(
		db: &mut AsyncPgConnection,
		server: Uuid,
		at: Option<Timestamp>,
	) -> Result<Vec<Status>> {
		use crate::schema::statuses::dsl::*;

		let cutoff = at.unwrap_or_else(Timestamp::now);
		let floor = cutoff.checked_sub(GRACE_LOOKBACK).unwrap_or(Timestamp::MIN);
		statuses
			.select(Status::as_select())
			.filter(
				server_id
					.eq(server)
					.and(id.ne(Uuid::nil()))
					.and(created_at.le(jiff_diesel::Timestamp::from(cutoff)))
					.and(created_at.ge(jiff_diesel::Timestamp::from(floor))),
			)
			.distinct_on(source)
			.order((source, created_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The most recent status for `server` that carries a version — wider
	/// than the live 7-day window. Used for the status card's headline
	/// version, which should reflect the last version a server reported even
	/// if it's currently down (and hence has no recent status). Capped at
	/// [`VERSION_LOOKBACK_SQL`]; see [`GRACE_LOOKBACK_SQL`] for why the read
	/// can't be left unbounded.
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
					.and(created_at.ge(diesel::dsl::sql(VERSION_LOOKBACK_SQL)))
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

	/// This one status's server-wide detail, for a caller that has a single
	/// status in hand rather than a server's set of sources. Prefer
	/// [`MergedDetail::from_statuses`] where every source is available: a
	/// figure this push doesn't carry is one another source may.
	pub fn detail(&self) -> MergedDetail {
		MergedDetail(self.extra.as_object().cloned().unwrap_or_default())
	}

	pub fn platform(&self) -> Option<String> {
		self.detail().platform()
	}

	pub fn postgres_version(&self) -> Option<String> {
		self.detail().postgres_version()
	}

	/// Node.js runtime version the server reported in its status payload
	/// (`nodeVersion` extra), if present. Preferred over scraping the device
	/// connection's User-Agent (see [`crate::devices::DeviceConnection::nodejs_version`]),
	/// which only reflects whichever transport happened to set that header.
	pub fn node_version(&self) -> Option<String> {
		self.detail().node_version()
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

/// A server's server-wide detail resolved across all the sources reporting on
/// it: for each key, the value from the most recent source that carries it.
///
/// Sources don't all report the same fields — only bestool reports
/// `bestoolVersion`, while a legacy Tamanu push carries neither that nor
/// `pgVersion` — so reading the figures off whichever source pushed last
/// makes them blink out as sources interleave. Falling through to an older
/// source per key holds each figure at its last reported value instead.
// spec: FIG#sourcing
#[derive(Debug, Clone, Default)]
pub struct MergedDetail(serde_json::Map<String, serde_json::Value>);

impl MergedDetail {
	/// Fold each source's report — `(when it was reported, what it carried)`,
	/// in any order — into the resolved detail. Newer reports win per key; a
	/// key absent from the newest falls through to the newest report that
	/// has it.
	///
	/// Which reports are passed sets what this sees. The live path hands it
	/// every source's current report (see
	/// [`crate::reported_detail::ReportedDetail`]); the point-in-time path
	/// hands it each source's latest push at-or-before a moment.
	pub fn from_reports<'a>(
		reports: impl IntoIterator<Item = (Timestamp, &'a serde_json::Value)>,
	) -> Self {
		// Callers' orderings vary and none of them are chronological —
		// latest_per_source_at orders by source name, the table read by
		// whatever the scan yields. "Newest wins" silently reading as "last
		// row wins" would be wrong in a way nothing would surface.
		let mut ordered: Vec<(Timestamp, &serde_json::Value)> = reports.into_iter().collect();
		ordered.sort_by_key(|(at, _)| *at);

		let mut merged = serde_json::Map::new();
		for (_, extra) in ordered {
			let Some(obj) = extra.as_object() else {
				continue;
			};
			for (key, value) in obj {
				if value.is_null() {
					continue;
				}
				merged.insert(key.clone(), value.clone());
			}
		}
		Self(merged)
	}

	/// [`Self::from_reports`] over one server's statuses.
	pub fn from_statuses(statuses: &[Status]) -> Self {
		Self::from_reports(statuses.iter().map(|st| (st.created_at, &st.extra)))
	}

	pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
		self.0.get(key)
	}

	/// The resolved detail as a JSON object, for handing to a client that
	/// wants to read fields canopy doesn't derive figures from.
	pub fn into_json(self) -> serde_json::Value {
		serde_json::Value::Object(self.0)
	}

	fn string(&self, key: &str) -> Option<String> {
		self.get(key)
			.and_then(|v| v.as_str())
			.map(ToOwned::to_owned)
	}

	/// The operating system the server runs, as it reports it — the name,
	/// qualified by the version when it reports one.
	///
	/// A server that reports neither falls back to what the PostgreSQL
	/// version banner gives away: it names its build toolchain, which
	/// distinguishes a Windows build from any other but says nothing finer.
	/// So an unreported platform degrades to the family rather than to
	/// nothing.
	// spec: FIG#figures
	pub fn platform(&self) -> Option<String> {
		if let Some(name) = self.string("osName") {
			return Some(match self.string("osVersion") {
				Some(version) => format!("{name} {version}"),
				None => name,
			});
		}

		self.string("pgVersion").map(|pg| {
			if pg.contains("Visual C++") || pg.contains("windows") {
				"Windows"
			} else {
				"Linux"
			}
			.into()
		})
	}

	pub fn postgres_version(&self) -> Option<String> {
		self.get("pgVersion")
			.and_then(|pg| pg.as_str())
			.and_then(|pg| pg.split_ascii_whitespace().nth(1))
			.map(|vers| vers.trim_end_matches(',').into())
	}

	pub fn node_version(&self) -> Option<String> {
		self.string("nodeVersion")
	}

	pub fn timezone(&self) -> Option<String> {
		self.string("timezone")
	}

	/// Whether the server runs Munin. Absent when no source has reported the
	/// flag — which is not the same as reporting that it doesn't.
	// spec: SVC#munin-link
	pub fn munin(&self) -> Option<bool> {
		self.get("munin").and_then(|v| v.as_bool())
	}

	/// Version of bestool itself, which it reports alongside the rest of its
	/// server-wide detail. Absent for a server no bestool reports on.
	// spec: FIG#figures
	pub fn bestool_version(&self) -> Option<String> {
		self.string("bestoolVersion")
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
