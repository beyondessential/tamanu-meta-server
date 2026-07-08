use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use commons_types::{
	server::{
		cards::{FacilityServerStatus, ServerGroupCard},
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
		.routes(routes!(check_attention))
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
	// Operator-silenced healthchecks don't count toward member health.
	let member_groups: Vec<(Uuid, Option<Uuid>)> =
		servers.iter().map(|s| (s.id, s.group_id)).collect();
	let silenced =
		database::silenced_refs::silenced_health_checks_for_servers(&mut conn, &member_groups)
			.await?;
	let no_silences = BTreeSet::new();

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
			let silenced_checks = silenced.get(&s.id).unwrap_or(&no_silences);
			FacilityServerStatus {
				id: s.id,
				name: s.name.clone().unwrap_or_default(),
				up,
				health: st
					.map(|s| s.health_state_ignoring(silenced_checks))
					.unwrap_or_default(),
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

/// One server whose latest status reports [`CheckAttentionData::check`],
/// for [`check_attention`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CheckAttentionServerData {
	/// The server's id — the UI links to `/servers/{server_id}`.
	pub server_id: Uuid,
	/// The server's display name; empty string when the server has none.
	pub server_name: String,
	/// The server's group id, if it belongs to one — the UI links to
	/// `/groups/{group_id}`.
	pub group_id: Option<Uuid>,
	/// The server's group name, if it belongs to one.
	pub group_name: Option<String>,
	/// The check's result on this server's latest status. The UI shows
	/// warning/failed/broken servers by default and puts passed/skipped
	/// ones behind a "show healthy" toggle.
	pub result: CheckResult,
	/// The check's full `health[]` entry from this server's latest status,
	/// verbatim (including the `check`/`healthy`/`result` keys), so the
	/// row can expand to the same per-check detail the server page shows.
	pub data: serde_json::Value,
	/// When this check started failing on this server: the `first_seen`
	/// of the still-active issue canopy filed at `(status,
	/// health/<check>)` when the check degraded. `None` for servers
	/// currently reporting the check healthy, and for failing servers
	/// with no active issue on file (e.g. the issue was
	/// operator-resolved, or the ref is silenced so nothing was filed).
	pub failing_since: Option<Timestamp>,
	/// When the reporting status was recorded.
	pub status_created_at: Timestamp,
}

/// Request body for [`check_attention`].
#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckAttentionArgs {
	/// The healthcheck name to look up, exactly as reported by devices in
	/// `health[].check` (an arbitrary, device/plugin-defined string).
	pub check: String,
}

/// Response for [`check_attention`]: the queried check's catalog policy
/// (if it has one yet) and every live server whose latest status reports
/// it, failing or healthy.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CheckAttentionData {
	/// The check name that was queried, echoed back so the page can
	/// render its heading without re-decoding the request.
	pub check: String,
	/// The most urgent configured policy ceiling for this check across
	/// the sources that report it, or `None` if no server has ever
	/// reported it yet (so it has no catalog row).
	#[schema(value_type = Option<String>)]
	pub ceiling: Option<CheckResult>,
	/// Whether any source's policy for this check escalates its
	/// effective failures.
	pub escalates: bool,
	/// Every live server whose latest status reports this check, at any
	/// result, ordered as a TODO list: failed, warning, broken, passed,
	/// skipped (most urgent first), then by group name then server name.
	/// The client filters out the passed/skipped tail unless the "show
	/// healthy" toggle is on.
	pub servers: Vec<CheckAttentionServerData>,
}

/// Rank used to order [`CheckAttentionServerData::result`] most-urgent
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

/// List the servers whose latest status reports one named healthcheck.
///
/// Everything the per-healthcheck page needs: the catalog's configured
/// severity for `check` (if any) plus every live server whose **latest**
/// status reports it — the current, real-time picture, not a history of
/// past issues/events, though each failing server carries a
/// `failing_since` timestamp derived from its active issue. This is the
/// data behind the `/healthchecks/:check` "who's affected" page, which
/// doubles as an operator TODO list and as a way to correlate servers
/// sharing the same issue during a fleet-wide incident.
#[utoipa::path(
	post,
	path = "/check_attention",
	operation_id = "status_check_attention",
	tag = "statuses",
	request_body = CheckAttentionArgs,
	responses(
		(status = 200, description = "The check's catalog severity and the servers currently reporting it.", body = CheckAttentionData),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn check_attention(
	State(state): State<AppState>,
	Json(args): Json<CheckAttentionArgs>,
) -> Result<Json<CheckAttentionData>> {
	let mut conn = state.db_read.get().await?;
	let reporting = Status::reporting_check_with_servers(&mut conn, &args.check).await?;

	let group_ids: Vec<Uuid> = reporting
		.iter()
		.filter_map(|(s, _)| s.group_id)
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	let group_names: HashMap<Uuid, String> = ServerGroup::list_by_ids(&mut conn, &group_ids)
		.await?
		.into_iter()
		.map(|g| (g.id, g.name))
		.collect();

	// "Failing since" comes from the issue the public-server status
	// handler files at `(<source>, health/<check>)` when a check degrades:
	// an active issue's first_seen is exactly when the current failure
	// streak began (recoveries close the issue, so a re-failure starts a
	// fresh one). This page correlates by check name across whichever
	// source reports it, so the lookup ignores the source.
	let server_ids: Vec<Uuid> = reporting.iter().map(|(s, _)| s.id).collect();
	let failing_since: HashMap<Uuid, Timestamp> =
		Issue::list_by_ref(&mut conn, &format!("health/{}", args.check), &server_ids)
			.await?
			.into_iter()
			.filter(|issue| issue.active)
			.filter_map(|issue| issue.server_id.map(|sid| (sid, issue.first_seen)))
			.collect();

	let mut servers: Vec<CheckAttentionServerData> = reporting
		.into_iter()
		.filter_map(|(server, status)| {
			let (result, entry) = status.check_entry(&args.check)?;
			let group_name = server.group_id.and_then(|g| group_names.get(&g).cloned());
			Some(CheckAttentionServerData {
				server_id: server.id,
				server_name: server.name.clone().unwrap_or_default(),
				group_id: server.group_id,
				group_name,
				result,
				data: serde_json::Value::Object(entry),
				failing_since: failing_since.get(&server.id).copied(),
				status_created_at: status.created_at,
			})
		})
		.collect();
	servers.sort_by(|a, b| {
		check_result_rank(a.result)
			.cmp(&check_result_rank(b.result))
			.then_with(|| a.group_name.cmp(&b.group_name))
			.then_with(|| a.server_name.cmp(&b.server_name))
	});

	let policies = CheckPolicy::get_by_name(&mut conn, &args.check).await?;
	let ceiling = policies
		.iter()
		.map(|row| row.ceiling)
		.min_by_key(|c| c.urgency_rank());
	let escalates = policies.iter().any(|row| row.escalates);

	Ok(Json(CheckAttentionData {
		check: args.check,
		ceiling,
		escalates,
		servers,
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
	/// Legacy overall self-reported health flag. Being phased out in favor
	/// of `health_state`; new integrations should not rely on it.
	pub healthy: bool,
	/// Overall health rollup for this push, derived from its individual
	/// health checks (falling back to the legacy `healthy` flag). A single
	/// failing check can't be masked by an otherwise-healthy overall
	/// report.
	pub health_state: commons_types::status::HealthState,
	/// Raw per-check health results as reported in this push.
	pub health: serde_json::Value,
	/// Additional unstructured data reported alongside this push, for
	/// fields not yet promoted to a named field on this response.
	pub extra: serde_json::Value,
	/// Operators identified as connected to the server as of this push,
	/// with display name and profile picture filled in where known. Not
	/// filtered by recency — reflects this specific point-in-time snapshot.
	pub operators: Vec<OperatorPresence>,
	/// For each currently-unhealthy check in this push, the severity it
	/// would be filed at if it turned into an issue. Healthy checks are
	/// omitted. An unhealthy check with no severity listed here should be
	/// treated as a default (warning-level) severity.
	pub check_severities: std::collections::HashMap<String, commons_types::issue::Severity>,
	/// Check names currently silenced for this server (at server or
	/// group scope). These don't count toward `health_state` and the UI
	/// renders them with its skipped affordance. Reflects the silence
	/// list as of the request, not as of the snapshot's push.
	pub silenced_checks: BTreeSet<String>,
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

	// Compute the per-unhealthy-check severity the rules engine would
	// file at given this push. Healthy checks are omitted; the UI
	// renders them with its 'passing' affordance regardless.
	let check_severities = compute_check_severities(&mut conn, &server, &status).await?;
	// Operator-silenced healthchecks present as skipped and don't count
	// toward the rollup, even on historical snapshots — a silence
	// expresses current operator intent about the check, not the push.
	let silenced_checks = database::silenced_refs::silenced_health_checks_for_server(
		&mut conn,
		server.id,
		server.group_id,
	)
	.await?;
	let health_state = status.health_state_ignoring(&silenced_checks);
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
		healthy: status.healthy,
		health_state,
		health: status.health,
		extra: status.extra,
		operators,
		check_severities,
		silenced_checks,
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
/// resolved tag map, expressed as the severity ingestion would file.
/// Mirrors the public-server ingestion path (`file_health_events`) so
/// the UI displays what *would* be filed. Broken checks aren't included
/// — they file at a fixed Warning and the UI renders them from the
/// result directly.
async fn compute_check_severities(
	conn: &mut database::diesel_async::AsyncPgConnection,
	server: &Server,
	status: &Status,
) -> commons_errors::Result<std::collections::HashMap<String, commons_types::issue::Severity>> {
	use commons_types::status::CheckResult;
	use database::check_policies::{CheckPolicy, EvaluationContext};

	let Some(arr) = status.health.as_array() else {
		return Ok(Default::default());
	};
	// Walk the health array once, collect (check_name, result,
	// check_extra) for every warning/failed entry. Other results don't
	// drive the rules engine on the ingestion path either, so we skip
	// them. Like ingestion, the normalised result is injected so rules
	// see a uniform `check.result` even on legacy stored rows.
	let mut failing: Vec<(
		String,
		CheckResult,
		serde_json::Map<String, serde_json::Value>,
	)> = Vec::new();
	for raw in arr {
		let Some(obj) = raw.as_object() else { continue };
		let Some(check_name) = obj.get("check").and_then(|v| v.as_str()) else {
			continue;
		};
		let Some(result) = CheckResult::from_entry(obj) else {
			continue;
		};
		if !matches!(result, CheckResult::Warning | CheckResult::Failed) {
			continue;
		}
		let mut extra = obj.clone();
		extra.remove("check");
		extra.remove("healthy");
		extra.insert(
			"result".into(),
			serde_json::Value::String(result.to_string()),
		);
		failing.push((check_name.to_string(), result, extra));
	}
	if failing.is_empty() {
		return Ok(Default::default());
	}

	let tag_map = server.tags_merged_with_group(conn).await?;
	let tags: std::collections::HashMap<String, serde_json::Value> = tag_map
		.0
		.into_iter()
		.map(|(k, v)| (k, serde_json::Value::String(v)))
		.collect();
	let empty_map = serde_json::Map::new();
	let status_extra = status.extra.as_object().unwrap_or(&empty_map);

	let mut out = std::collections::HashMap::with_capacity(failing.len());
	for (name, result, check_extra) in failing {
		let ctx = EvaluationContext {
			status_extra,
			check_extra: &check_extra,
			tags: &tags,
		};
		let graded = CheckPolicy::apply(conn, &status.source, &name, result, &ctx).await?;
		let sev = match graded.effective {
			CheckResult::Failed if graded.escalates => commons_types::issue::Severity::Critical,
			CheckResult::Failed => commons_types::issue::Severity::Error,
			CheckResult::Warning | CheckResult::Broken => commons_types::issue::Severity::Warning,
			CheckResult::Passed | CheckResult::Skipped => continue,
		};
		out.insert(name, sev);
	}
	Ok(out)
}
