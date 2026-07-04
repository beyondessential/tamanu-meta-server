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
	status::{OperatorPresence, ShortStatus},
	version::VersionStr,
};
use database::{
	devices::DeviceConnection, server_groups::ServerGroup, servers::Server, statuses::Status,
	tailscale_users::TailscaleUser as CachedTailscaleUser, versions::Version,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, ToSchema)]
pub struct LiveVersionsBracket {
	pub min: VersionStr,
	pub max: VersionStr,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SummaryData {
	pub bracket: LiveVersionsBracket,
	#[schema(value_type = Vec<(u64, u64)>)]
	pub releases: BTreeSet<(u64, u64)>,
	#[schema(value_type = Vec<VersionStr>)]
	pub versions: BTreeSet<VersionStr>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerDetailsData {
	pub id: String,
	pub name: String,
	pub kind: String,
	pub rank: String,
	pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServerStatusData {
	pub up: String,
	pub updated_at: Option<String>,
	pub version: Option<String>,
	pub platform: Option<String>,
	pub postgres: Option<String>,
	pub nodejs: Option<String>,
	pub timezone: Option<String>,
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(summary))
		.routes(routes!(server_grouped_ids))
		.routes(routes!(group_details))
		.routes(routes!(snapshot))
}

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

/// Server group ids bucketed by their **highest-ranked member's** rank.
/// `ServerRank::Production` outranks `Clone`, `Demo`, `Test`, `Dev` in that
/// order. Groups whose members are all unranked don't appear in the response
/// at all (the status page intentionally hides them — they're typically dev
/// scratch).
///
/// Within each bucket, groups are ordered alphabetically by name.
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

#[derive(Deserialize, ToSchema)]
pub struct GroupDetailsArgs {
	pub server_group_id: Uuid,
}

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
				health: st.map(|s| s.health_state()).unwrap_or_default(),
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

/// What the UI needs to render a status snapshot — the curated
/// fields ServerDetail already shows (so the modal/section can look
/// like the rest of the app) plus the new `healthy` / `health` and
/// the raw `extra` blob for forward-compat as the contract expands.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatusSnapshotData {
	pub id: Uuid,
	pub created_at: Timestamp,
	pub server_id: Uuid,
	pub device_id: Option<Uuid>,
	pub version: Option<VersionStr>,
	pub version_distance: Option<u64>,
	pub min_chrome_version: Option<u32>,
	pub platform: Option<String>,
	pub postgres: Option<String>,
	pub nodejs: Option<String>,
	pub timezone: Option<String>,
	/// Raw legacy top-level self-report. Being retired from the wire
	/// (absent ⇒ true on ingestion); UI display should use
	/// `health_state` instead.
	pub healthy: bool,
	/// Rollup over the per-check results (and the legacy top-level
	/// flag) — same derivation as the status-dot border. The UI's
	/// headline chip uses this so a failing check can't hide behind
	/// a self-reported (or defaulted) top-level `healthy: true`.
	pub health_state: commons_types::status::HealthState,
	pub health: serde_json::Value,
	pub extra: serde_json::Value,
	/// Identified operators connected as of this push, from the
	/// `external_users` check, with display info filled from the
	/// `tailscale_users` cache. Not freshness-gated — a snapshot is
	/// explicitly "as of" a point in time.
	pub operators: Vec<OperatorPresence>,
	/// For each unhealthy check on this push, the severity the
	/// catalog + rules engine would file at. Healthy checks are
	/// absent; absence on an unhealthy check means the catalog has
	/// no row yet (treat as the default Warning) — the UI surfaces
	/// the explicit severity when one is known so operators see
	/// all five levels, not just the legacy "warning/error" pair
	/// derived from `healthy`.
	pub check_severities: std::collections::HashMap<String, commons_types::issue::Severity>,
}

#[derive(Deserialize, ToSchema)]
pub struct SnapshotArgs {
	pub server_id: Uuid,
	/// When the snapshot should be "as of". Returns the most recent
	/// status row with `created_at <= at`. Omit (or `null`) to get
	/// the latest status (no time bound).
	#[serde(default)]
	pub at: Option<Timestamp>,
}

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
	let check_severities = compute_check_severities(&mut conn, args.server_id, &status).await?;
	let health_state = status.health_state();
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

/// For every warning/failed check on `status`, resolve the catalog +
/// rules severity given the snapshot's actual extras and the server's
/// resolved tag map. Mirrors the public-server ingestion path
/// (`file_health_events`) so the UI displays what *would* be filed.
/// Broken checks aren't included — they file at a fixed Warning and
/// the UI renders them from the result directly; passed/skipped file
/// nothing.
async fn compute_check_severities(
	conn: &mut database::diesel_async::AsyncPgConnection,
	server_id: Uuid,
	status: &Status,
) -> commons_errors::Result<std::collections::HashMap<String, commons_types::issue::Severity>> {
	use commons_types::status::CheckResult;
	use database::healthcheck_severities::{EvaluationContext, HealthcheckSeverity};
	use database::servers::Server as DbServer;

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

	let server = DbServer::get_by_id(conn, server_id).await?;
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
		let sev = HealthcheckSeverity::severity_for(conn, &name, result, &ctx).await?;
		out.insert(name, sev);
	}
	Ok(out)
}

// Touch `Server` so the unused-import warning doesn't fire when this module
// is compiled standalone in some configurations.
#[allow(dead_code)]
fn _server_touch(_s: Server) {}
