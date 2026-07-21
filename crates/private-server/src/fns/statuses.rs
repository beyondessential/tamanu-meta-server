use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use commons_types::{
	server::{
		cards::{FacilityServerStatus, ServerGroupCard},
		kind::ServerKind,
		rank::ServerRank,
	},
	status::{CheckResult, OperatorPresence, ShortStatus},
	version::VersionStr,
};
use database::{
	check_policies::CheckPolicy, devices::DeviceConnection, issues::Issue,
	server_groups::ServerGroup, servers::Server, statuses::Status,
	tailscale_users::TailscaleUser as CachedTailscaleUser, versions::Version,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

/// The lowest and highest software version currently reported by any
/// production server.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, ToSchema)]
pub struct LiveVersionsBracket {
	/// Oldest version currently reported by a production server.
	pub min: VersionStr,
	/// Newest version currently reported by a production server.
	pub max: VersionStr,
}

/// Fleet-wide summary of software versions currently running in production.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SummaryData {
	/// Lowest and highest version currently reported by a production
	/// server.
	pub bracket: LiveVersionsBracket,
	/// Distinct major.minor release lines currently observed in
	/// production, each as a `[major, minor]` pair.
	#[schema(value_type = Vec<(u64, u64)>)]
	pub releases: BTreeSet<(u64, u64)>,
	/// Every distinct full version string currently observed in
	/// production, in ascending order.
	#[schema(value_type = Vec<VersionStr>)]
	pub versions: BTreeSet<VersionStr>,
}

/// Basic identifying details for a server. Currently unused by any
/// endpoint in this API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerDetailsData {
	/// Server id.
	pub id: String,
	/// Server display name.
	pub name: String,
	/// Server kind (deployment type).
	pub kind: String,
	/// Server rank (e.g. production, test, dev).
	pub rank: String,
	/// Server hostname or address.
	pub host: String,
}

/// A simplified snapshot of a server's reported status. Currently unused by
/// any endpoint in this API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerStatusData {
	/// Up/down state, as a short status string.
	pub up: String,
	/// When this status was last updated, as a formatted string.
	pub updated_at: Option<String>,
	/// Reported software version.
	pub version: Option<String>,
	/// Reported operating system platform.
	pub platform: Option<String>,
	/// Reported database engine version.
	pub postgres: Option<String>,
	/// Reported runtime version.
	pub nodejs: Option<String>,
	/// Reported system timezone.
	pub timezone: Option<String>,
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(summary))
		.routes(routes!(server_grouped_ids))
		.routes(routes!(group_details))
		.routes(routes!(snapshot))
		.routes(routes!(check_detail))
}

/// Get a fleet-wide summary of software versions running in production.
///
/// Looks at the most recent status reported by every server ranked as
/// production (within the last 7 days) and returns the range of versions
/// seen, the distinct release lines, and every distinct exact version.
#[utoipa::path(
	post,
	path = "/summary",
	operation_id = "status_summary",
	tag = "statuses",
	responses(
		(status = 200, body = SummaryData),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn summary(State(state): State<AppState>) -> Result<Json<SummaryData>> {
	let mut conn = state.db_read.get().await?;

	let versions: BTreeSet<VersionStr> = Status::production_versions(&mut conn)
		.await?
		.into_iter()
		.collect();

	let bracket = LiveVersionsBracket {
		min: versions.first().cloned().unwrap_or_default(),
		max: versions.last().cloned().unwrap_or_default(),
	};
	let releases = versions
		.iter()
		.map(|v| (v.0.major, v.0.minor))
		.collect::<BTreeSet<_>>();

	Ok(Json(SummaryData {
		bracket,
		releases,
		versions,
	}))
}

/// List server group ids, bucketed by rank.
///
/// Each group is bucketed under the highest rank held by any of its member
/// servers (production outranks clone, which outranks demo, then test,
/// then dev). Groups whose members are all unranked are omitted entirely.
/// Within each rank bucket, groups are ordered alphabetically by name.
#[utoipa::path(
	post,
	path = "/server_grouped_ids",
	tag = "statuses",
	responses(
		(status = 200, description = "Server group IDs grouped by highest-ranked member's rank.", body = BTreeMap<ServerRank, Vec<Uuid>>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn server_grouped_ids(
	State(state): State<AppState>,
) -> Result<Json<BTreeMap<ServerRank, Vec<Uuid>>>> {
	let mut conn = state.db_read.get().await?;
	let groups = ServerGroup::list_all(&mut conn).await?;
	if groups.is_empty() {
		return Ok(Json(BTreeMap::new()));
	}
	let group_ids: Vec<Uuid> = groups.iter().map(|g| g.id).collect();
	let top_rank = ServerGroup::highest_member_ranks(&mut conn, &group_ids).await?;

	let mut by_rank: BTreeMap<ServerRank, Vec<(String, Uuid)>> = BTreeMap::new();
	for g in groups {
		if let Some(rank) = top_rank.get(&g.id) {
			by_rank.entry(*rank).or_default().push((g.name, g.id));
		}
	}
	let map: BTreeMap<ServerRank, Vec<Uuid>> = by_rank
		.into_iter()
		.map(|(rank, mut list)| {
			list.sort_by(|a, b| a.0.cmp(&b.0));
			(rank, list.into_iter().map(|(_, id)| id).collect())
		})
		.collect();
	Ok(Json(map))
}

/// Identifies the server group whose status details to fetch.
#[derive(Deserialize, ToSchema)]
pub struct GroupDetailsArgs {
	/// Id of the server group to fetch details for.
	pub server_group_id: Uuid,
}

/// Get a status card for a server group.
///
/// Returns the group's identity, its headline version and how far behind
/// the latest published release that version is, and a per-member summary
/// (up/down state, health, connected operators, rank, kind) for every
/// server in the group. Returns 404 if the group doesn't exist.
#[utoipa::path(
	post,
	path = "/group_details",
	tag = "statuses",
	request_body = GroupDetailsArgs,
	responses(
		(status = 200, body = ServerGroupCard),
		(status = 404, body = ProblemDetailsSchema),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn group_details(
	State(state): State<AppState>,
	Json(args): Json<GroupDetailsArgs>,
) -> Result<Json<ServerGroupCard>> {
	let mut conn = state.db_read.get().await?;
	let group = ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;
	let servers = group.list_servers(&mut conn).await?;

	let latest_version = Version::get_latest_matching(&mut conn, "*".parse()?)
		.await?
		.as_semver();

	let server_ids: Vec<Uuid> = servers.iter().map(|s| s.id).collect();
	let status_map: HashMap<Uuid, Status> = Status::latest_for_servers(&mut conn, &server_ids)
		.await?
		.into_iter()
		.map(|s| (s.server_id, s))
		.collect();
	// Member health rolls up current check state across every source
	// (silenced checks already skipped in the rollup).
	let member_groups: Vec<(Uuid, Option<Uuid>)> =
		servers.iter().map(|s| (s.id, s.group_id)).collect();
	let member_health =
		database::issues::health_from_check_state(&mut conn, &member_groups).await?;

	// The card's headline version is the cached last reported version of the
	// group's canonical member (highest rank, then highest kind), maintained by
	// the `statuses` trigger and `ServerGroup::recompute_version`. The distance
	// is computed against the latest published version.
	let card_version = group.effective_version.clone();
	let version_distance = card_version
		.as_ref()
		.map(|v| database::statuses::version_distance(&v.0, &latest_version));

	let mut members: Vec<FacilityServerStatus> = servers
		.into_iter()
		.map(|s| {
			let st = status_map.get(&s.id);
			let up = st.map(|s| s.short_status()).unwrap_or_default();
			// Active presence only: a server that's stopped reporting may
			// well still have those sessions, but we can't assert "in the
			// server right now" from a stale push.
			let operators = match up {
				ShortStatus::Up | ShortStatus::Blip => {
					st.map(|s| s.operators()).unwrap_or_default()
				}
				_ => Vec::new(),
			};
			FacilityServerStatus {
				id: s.id,
				name: s.name.clone().unwrap_or_default(),
				up,
				health: member_health.get(&s.id).copied().unwrap_or_default(),
				operators,
				rank: s.rank,
				kind: s.kind,
			}
		})
		.collect();
	enrich_operators(
		&mut conn,
		members.iter_mut().flat_map(|m| m.operators.iter_mut()),
	)
	.await?;

	Ok(Json(ServerGroupCard {
		id: group.id,
		name: group.name,
		notes: group.notes,
		version: card_version,
		version_distance,
		members,
	}))
}

/// One server whose latest status reports [`CheckDetailData::check`],
/// for [`check_detail`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CheckDetailServerData {
	/// The server's id — the UI links to `/servers/{server_id}`.
	pub server_id: Uuid,
	/// The server's display name; empty string when the server has none.
	pub server_name: String,
	/// The server's group id, if it belongs to one — the UI links to
	/// `/groups/{group_id}`.
	pub group_id: Option<Uuid>,
	/// The server's group name, if it belongs to one.
	pub group_name: Option<String>,
	/// The server's rank, for the standard rank-bucket grouping.
	pub rank: Option<ServerRank>,
	/// The server's kind, for the standard within-rank ordering.
	pub kind: ServerKind,
	/// The check's observed result on its latest report. The UI shows
	/// warning/failed/broken servers by default and puts passed/skipped
	/// ones behind a "show healthy" toggle.
	pub result: CheckResult,
	/// The check's own fields from its latest report, verbatim, so the
	/// row can expand to the same per-check detail the server page shows.
	pub data: serde_json::Value,
	/// When the check's current degradation streak began. `None` for
	/// servers currently reporting the check healthy.
	pub failing_since: Option<Timestamp>,
	/// When the check state last updated (the check's latest report).
	pub status_created_at: Timestamp,
	/// The state's stability record (observation counters, transition
	/// ring, hour-of-week duty profile, derived flap statistics). `None`
	/// for states that predate stability recording.
	pub stability: Option<database::stability::StabilityData>,
}

/// A group-scoped state of this check — a condition Canopy determines
/// about the group's control plane (backup health and the like) — for
/// the group's section of the check detail list.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CheckDetailGroupData {
	/// The group's id — the UI links to `/groups/{group_id}`.
	pub group_id: Uuid,
	/// The group's display name.
	pub group_name: String,
	/// Rank bucket for display: the highest rank held by any member
	/// server, mirroring the status page's group bucketing.
	pub rank: Option<ServerRank>,
	/// The check's observed result on its latest filing.
	pub result: CheckResult,
	/// The check's own fields from its latest filing, verbatim.
	pub data: serde_json::Value,
	/// When the current degradation streak began; `None` while healthy.
	pub failing_since: Option<Timestamp>,
	/// When the check state last updated.
	pub status_created_at: Timestamp,
	/// The state's stability record; `None` for states that predate
	/// stability recording.
	pub stability: Option<database::stability::StabilityData>,
}

/// The canopy-wide state of this check (self-monitoring), if any.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CheckDetailCanopyData {
	/// The check's observed result on its latest filing.
	pub result: CheckResult,
	/// The check's own fields from its latest filing, verbatim.
	pub data: serde_json::Value,
	/// When the current degradation streak began; `None` while healthy.
	pub failing_since: Option<Timestamp>,
	/// When the check state last updated.
	pub status_created_at: Timestamp,
	/// The state's stability record; `None` for states that predate
	/// stability recording.
	pub stability: Option<database::stability::StabilityData>,
}

/// Request body for [`check_detail`].
#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckDetailArgs {
	/// The source that reports the check. A check's identity is the
	/// (source, check) pair — a same-named check from another source is
	/// a different check.
	pub source: String,
	/// The healthcheck name to look up, exactly as reported by devices in
	/// `health[].check` (an arbitrary, device/plugin-defined string).
	pub check: String,
}

/// Response for [`check_detail`]: the queried check's catalog policy
/// (if it has one yet) and every live server whose latest status reports
/// it, failing or healthy.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CheckDetailData {
	/// The source that was queried, echoed back with `check` so the page
	/// can render its heading without re-decoding the request.
	pub source: String,
	/// The check name that was queried.
	pub check: String,
	/// The configured policy ceiling for this (source, check), or `None`
	/// if the source has never reported it (so it has no catalog row).
	#[schema(value_type = Option<String>)]
	pub ceiling: Option<CheckResult>,
	/// Whether this check's policy escalates its effective failures.
	pub escalates: bool,
	/// Operator-authored documentation for this (source, check)
	/// (markdown), or `None` if nobody has written it yet.
	pub documentation: Option<String>,
	/// Every live server whose latest state from this source reports
	/// this check, at any result, ordered as a TODO list: failed,
	/// warning, broken, passed, skipped (most urgent first), then by
	/// group name then server name. The client filters out the
	/// passed/skipped tail unless the "show healthy" toggle is on.
	pub servers: Vec<CheckDetailServerData>,
	/// Group-scoped states of this check, ordered by group name. The
	/// client files each under its group in the list.
	pub groups: Vec<CheckDetailGroupData>,
	/// The canopy-wide state of this check, if canopy has ever filed one.
	pub canopy: Option<CheckDetailCanopyData>,
}

/// Rank used to order [`CheckDetailServerData::result`] most-urgent
/// first: failed, then warning, then broken, with the healthy tail
/// (passed, then skipped) last. Mirrors the private-web
/// `CHECK_RESULT_ORDER` display order.
fn check_result_rank(result: CheckResult) -> u8 {
	match result {
		CheckResult::Failed => 0,
		CheckResult::Warning => 1,
		CheckResult::Broken => 2,
		CheckResult::Passed => 3,
		CheckResult::Skipped => 4,
	}
}

/// List the servers whose check state reports one (source, check).
///
/// Everything the per-healthcheck page needs: the catalog's configured
/// policy for the (source, check) (if any) plus every live server's
/// current state for it — the real-time picture, with each degraded row
/// carrying `failing_since` (the start of its current degradation
/// streak). This is the data behind the `/healthchecks/:source/:check`
/// "who's affected" page, which doubles as an operator TODO list and as
/// a way to correlate servers sharing the same issue during a
/// fleet-wide incident.
#[utoipa::path(
	post,
	path = "/check_detail",
	operation_id = "status_check_detail",
	tag = "statuses",
	request_body = CheckDetailArgs,
	responses(
		(status = 200, description = "The check's catalog policy and the servers currently reporting it.", body = CheckDetailData),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn check_detail(
	State(state): State<AppState>,
	Json(args): Json<CheckDetailArgs>,
) -> Result<Json<CheckDetailData>> {
	let mut conn = state.db_read.get().await?;
	let states = Issue::check_state_for_check(&mut conn, &args.source, &args.check).await?;

	// Live servers only: archived servers and canopy's own row never
	// appear on the check detail page.
	let server_ids: Vec<Uuid> = states
		.iter()
		.filter_map(|st| st.server_id)
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	let live: HashMap<Uuid, database::servers::Server> =
		database::servers::Server::get_by_ids(&mut conn, &server_ids)
			.await?
			.into_iter()
			.filter(|s| s.deleted_at.is_none() && s.id != Uuid::nil())
			.map(|s| (s.id, s))
			.collect();
	// Group names (and, for group-scoped states, rank buckets) cover both
	// the member servers' groups and the groups with their own state.
	let group_ids: Vec<Uuid> = live
		.values()
		.filter_map(|s| s.group_id)
		.chain(states.iter().filter_map(|st| st.server_group_id))
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	let group_names: HashMap<Uuid, String> = ServerGroup::list_by_ids(&mut conn, &group_ids)
		.await?
		.into_iter()
		.map(|g| (g.id, g.name))
		.collect();
	let scoped_group_ids: Vec<Uuid> = states
		.iter()
		.filter_map(|st| st.server_group_id)
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	let group_ranks = ServerGroup::highest_member_ranks(&mut conn, &scoped_group_ids).await?;

	let issue_ids: Vec<Uuid> = states.iter().map(|st| st.id).collect();
	let stability =
		database::stability::CheckStability::for_issue_ids(&mut conn, &issue_ids).await?;
	let now = jiff::Timestamp::now();

	// failing_since is the current degradation streak; recovered rows have
	// none. first_seen is the fallback for rows stamped before
	// degraded_since existed.
	let failing_since = |st: &Issue| {
		st.active
			.then_some(st.degraded_since.unwrap_or(st.first_seen))
	};

	let mut servers: Vec<CheckDetailServerData> = Vec::new();
	let mut groups: Vec<CheckDetailGroupData> = Vec::new();
	let mut canopy: Option<CheckDetailCanopyData> = None;
	for st in states {
		let Some(result) = st.observed_result else {
			continue;
		};
		let row_stability = stability
			.get(&st.id)
			.map(|row| database::stability::StabilityData::from_row(row, now));
		match (st.server_id, st.server_group_id) {
			(Some(sid), _) => {
				let Some(server) = live.get(&sid) else {
					continue;
				};
				servers.push(CheckDetailServerData {
					server_id: server.id,
					server_name: server.name.clone().unwrap_or_default(),
					group_id: server.group_id,
					group_name: server.group_id.and_then(|g| group_names.get(&g).cloned()),
					rank: server.rank,
					kind: server.kind,
					result,
					data: st.detail.clone().unwrap_or_else(|| serde_json::json!({})),
					failing_since: failing_since(&st),
					status_created_at: st.last_seen,
					stability: row_stability,
				});
			}
			(None, Some(gid)) => {
				groups.push(CheckDetailGroupData {
					group_id: gid,
					group_name: group_names.get(&gid).cloned().unwrap_or_default(),
					rank: group_ranks.get(&gid).copied(),
					result,
					data: st.detail.clone().unwrap_or_else(|| serde_json::json!({})),
					failing_since: failing_since(&st),
					status_created_at: st.last_seen,
					stability: row_stability,
				});
			}
			(None, None) => {
				canopy = Some(CheckDetailCanopyData {
					result,
					data: st.detail.clone().unwrap_or_else(|| serde_json::json!({})),
					failing_since: failing_since(&st),
					status_created_at: st.last_seen,
					stability: row_stability,
				});
			}
		}
	}
	servers.sort_by(|a, b| {
		check_result_rank(a.result)
			.cmp(&check_result_rank(b.result))
			.then_with(|| a.group_name.cmp(&b.group_name))
			.then_with(|| a.server_name.cmp(&b.server_name))
	});
	groups.sort_by(|a, b| a.group_name.cmp(&b.group_name));

	let policy = CheckPolicy::get(&mut conn, &args.source, &args.check).await?;

	Ok(Json(CheckDetailData {
		source: args.source,
		check: args.check,
		ceiling: policy.as_ref().map(|p| p.ceiling),
		escalates: policy.as_ref().is_some_and(|p| p.escalates),
		documentation: policy.and_then(|p| p.documentation),
		servers,
		groups,
		canopy,
	}))
}

/// A single status push from a server, as of a point in time, with derived
/// health and version information.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatusSnapshotData {
	/// Unique identifier for this status push.
	pub id: Uuid,
	/// When this status push was recorded.
	pub created_at: Timestamp,
	/// Id of the server this status was reported by.
	pub server_id: Uuid,
	/// Id of the device that sent this status push, if known.
	pub device_id: Option<Uuid>,
	/// Software version reported in this push.
	pub version: Option<VersionStr>,
	/// How many releases behind the latest published version this push's
	/// version is. Absent when there's no published version to compare
	/// against.
	pub version_distance: Option<u64>,
	/// Minimum embedded browser version required by this software version,
	/// if known.
	pub min_chrome_version: Option<u32>,
	/// Reported operating system platform.
	pub platform: Option<String>,
	/// Reported database engine version.
	pub postgres: Option<String>,
	/// Reported runtime version.
	pub nodejs: Option<String>,
	/// Reported system timezone.
	pub timezone: Option<String>,
	/// Additional unstructured data reported alongside this push, for
	/// fields not yet promoted to a named field on this response.
	pub extra: serde_json::Value,
	/// Operators identified as connected to the server as of this push,
	/// with display name and profile picture filled in where known. Not
	/// filtered by recency — reflects this specific point-in-time snapshot.
	pub operators: Vec<OperatorPresence>,
	/// The server's consolidated checks as of this snapshot: every source's
	/// checks, graded and classified, with silenced flags and the rolled-up
	/// health state — the same shape the live view uses.
	pub checks: commons_types::status::ConsolidatedChecks,
}

/// Selects a server and a point in time to fetch a status snapshot for.
#[derive(Deserialize, ToSchema)]
pub struct SnapshotArgs {
	/// Id of the server to fetch a status snapshot for.
	pub server_id: Uuid,
	/// Point in time the snapshot should be "as of". Returns the most
	/// recent status reported at or before this time. Omit (or send
	/// `null`) to get the latest status with no time bound.
	#[serde(default)]
	pub at: Option<Timestamp>,
}

/// Get a server's status as of a point in time.
///
/// Returns the most recent status push at or before `at` (or the latest
/// push overall, if `at` is omitted), enriched with version distance,
/// minimum browser version, connected operators, and per-check severities.
/// Returns `null` in the response body if the server has no recorded
/// status.
#[utoipa::path(
	post,
	path = "/snapshot",
	operation_id = "status_snapshot",
	tag = "statuses",
	security(("tailscale-user" = [])),
	request_body = SnapshotArgs,
	responses(
		(status = 200, body = Option<StatusSnapshotData>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn snapshot(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<SnapshotArgs>,
) -> Result<Json<Option<StatusSnapshotData>>> {
	let mut conn = state.db_read.get().await?;
	let status = match args.at {
		Some(at) => Status::at_time(&mut conn, args.server_id, at).await?,
		None => Status::latest_for_server(&mut conn, args.server_id).await?,
	};
	let Some(status) = status else {
		return Ok(Json(None));
	};
	let server = Server::get_by_id(&mut conn, args.server_id).await?;

	// If the deployment has no published versions yet, we just skip
	// the distance computation rather than 404'ing the whole
	// snapshot — the call still wants to surface everything else.
	let version_distance = match Version::get_latest_matching(&mut conn, "*".parse()?).await {
		Ok(v) => status.distance_from_version(&v.as_semver()),
		Err(_) => None,
	};
	// Prefer the Node.js version the server reported in its status payload
	// (`nodeVersion`). Fall back to scraping the *latest* device connection's
	// User-Agent — that metadata isn't versioned in lockstep with status
	// pushes, so looking it up "as of" a time would mostly mislead.
	let nodejs = match status.node_version() {
		Some(v) => Some(v),
		None => {
			if let Some(dev_id) = status.device_id {
				DeviceConnection::get_latest_from_device_ids(&mut conn, [dev_id].into_iter())
					.await?
					.into_iter()
					.next()
					.and_then(|d| d.nodejs_version())
			} else {
				None
			}
		}
	};
	let min_chrome_version = if let Some(ref v) = status.version {
		super::servers::compute_min_chrome_version(&mut conn, v).await
	} else {
		None
	};
	let timezone = status
		.extra("timezone")
		.and_then(|v| v.as_str().map(|s| s.to_string()));
	let platform = status.platform();
	let postgres = status.postgres_version();

	// The consolidated checks as of this snapshot: every source's most
	// recent report at-or-before `at`, re-graded through current policy.
	// The single `status` above is still used for the push's metadata
	// (version, platform, operators, etc.).
	let checks = consolidated_checks_at(&mut conn, &server, args.at).await?;
	let mut operators = status.operators();
	enrich_operators(&mut conn, operators.iter_mut()).await?;

	Ok(Json(Some(StatusSnapshotData {
		id: status.id,
		created_at: status.created_at,
		server_id: status.server_id,
		device_id: status.device_id,
		version: status.version,
		version_distance,
		min_chrome_version,
		platform,
		postgres,
		nodejs,
		timezone,
		extra: status.extra,
		operators,
		checks,
	})))
}

/// Fill `name`/`profile_pic` on operator entries from the
/// `tailscale_users` cache, in one batch lookup for the lot. Logins that
/// have never authenticated against canopy stay bare — the UI falls back
/// to a letter avatar.
pub(crate) async fn enrich_operators(
	conn: &mut database::diesel_async::AsyncPgConnection,
	operators: impl Iterator<Item = &mut OperatorPresence>,
) -> commons_errors::Result<()> {
	let mut ops: Vec<&mut OperatorPresence> = operators.collect();
	if ops.is_empty() {
		return Ok(());
	}
	let logins: Vec<String> = ops
		.iter()
		.map(|o| o.login.clone())
		.collect::<std::collections::BTreeSet<_>>()
		.into_iter()
		.collect();
	let login_refs: Vec<&str> = logins.iter().map(String::as_str).collect();
	let users = CachedTailscaleUser::by_logins(conn, &login_refs).await?;
	for op in &mut ops {
		if let Some(u) = users.get(&op.login) {
			op.name = Some(u.name.clone());
			op.profile_pic = u.profile_pic.clone();
		}
	}
	Ok(())
}

/// For every warning/failed check on `status`, resolve the policy +
/// rules grading given the snapshot's actual extras and the server's
/// resolved tag map: the effective result ingestion would file. Mirrors
/// the public-server ingestion path (`file_health_events`) so the UI
/// displays what *would* be filed. Broken checks aren't included — they
/// file as warnings and the UI renders them from the result directly.
/// Reconstruct a server's consolidated checks as of a point in time (or
/// latest) from status history: each source's most recent report at-or-
/// before `at`, every check re-graded through current policy — the same
/// shape the live path builds from current state. The point-in-time side
/// of the consolidated checks view.
async fn consolidated_checks_at(
	conn: &mut database::diesel_async::AsyncPgConnection,
	server: &Server,
	at: Option<Timestamp>,
) -> commons_errors::Result<commons_types::status::ConsolidatedChecks> {
	use commons_types::status::{CheckResult, ConsolidatedCheck, ConsolidatedChecks, HealthState};
	use database::check_policies::{CheckPolicy, EvaluationContext};

	let statuses = Status::latest_per_source_at(conn, server.id, at).await?;

	// Tags for rule evaluation, as private-server's other rule-eval sites
	// resolve them.
	let tag_map = server.tags_merged_with_group(conn).await?;
	let tags: std::collections::HashMap<String, serde_json::Value> = tag_map
		.0
		.into_iter()
		.map(|(k, v)| (k, serde_json::Value::String(v)))
		.collect();
	let decommissioned = CheckPolicy::decommissioned_pairs(conn).await?;

	let mut checks: Vec<ConsolidatedCheck> = Vec::new();
	for status in &statuses {
		let Some(arr) = status.health.as_array() else {
			continue;
		};
		let silenced = database::silenced_refs::silenced_health_checks_for_server(
			conn,
			server.id,
			server.group_id,
			&status.source,
		)
		.await?;
		let empty = serde_json::Map::new();
		let status_extra = status.extra.as_object().unwrap_or(&empty).clone();
		for raw in arr {
			let Some(obj) = raw.as_object() else { continue };
			let Some(name) = obj.get("check").and_then(|v| v.as_str()) else {
				continue;
			};
			if decommissioned.contains(&(status.source.clone(), name.to_string())) {
				continue;
			}
			let Some(observed) = CheckResult::from_entry(obj) else {
				continue;
			};
			// Re-grade this entry through current policy, mirroring ingestion:
			// the normalised result is injected so rules see a uniform
			// `check.result` even on legacy stored rows.
			let mut check_extra = obj.clone();
			check_extra.remove("check");
			check_extra.remove("healthy");
			check_extra.insert(
				"result".into(),
				serde_json::Value::String(observed.to_string()),
			);
			let ctx = EvaluationContext {
				status_extra: &status_extra,
				check_extra: &check_extra,
				tags: &tags,
			};
			let graded = CheckPolicy::apply_scoped(
				conn,
				&status.source,
				name,
				observed,
				&ctx,
				Some(server.id),
				server.group_id,
			)
			.await?;
			// The check's own detail fields, verbatim.
			let mut detail = obj.clone();
			detail.remove("check");
			detail.remove("healthy");
			detail.remove("result");
			checks.push(ConsolidatedCheck {
				silenced: silenced.contains(name),
				source: status.source.clone(),
				check: name.to_string(),
				observed: Some(observed),
				effective: graded.effective,
				detail: serde_json::Value::Object(detail),
			});
		}
	}
	checks.sort_by(|a, b| {
		a.effective
			.urgency_rank()
			.cmp(&b.effective.urgency_rank())
			.then_with(|| a.source.cmp(&b.source))
			.then_with(|| a.check.cmp(&b.check))
	});
	let health_state =
		HealthState::from_results(checks.iter().filter(|c| !c.silenced).map(|c| c.effective));
	Ok(ConsolidatedChecks {
		health_state,
		checks,
	})
}
