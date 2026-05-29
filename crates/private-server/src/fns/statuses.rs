use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::Json;
use axum::extract::State;
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use commons_types::{
	server::{
		cards::{FacilityServerStatus, ServerGroupCard},
		rank::ServerRank,
	},
	version::VersionStr,
};
use database::{
	devices::DeviceConnection, server_groups::ServerGroup, servers::Server, statuses::Status,
	versions::Version,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
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
	let mut conn = state.db.get().await?;

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
	let mut conn = state.db.get().await?;
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
	let mut conn = state.db.get().await?;
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

	// Pick a representative server to provide the card's headline version:
	// the one with the most recent status. Ties (no status anywhere) fall
	// back to the first server by name.
	let representative = servers
		.iter()
		.max_by_key(|s| status_map.get(&s.id).map(|st| st.created_at))
		.cloned();
	let rep_status = representative.as_ref().and_then(|s| status_map.get(&s.id));
	let version_distance = rep_status.and_then(|s| s.distance_from_version(&latest_version));

	let members = servers
		.into_iter()
		.map(|s| {
			let st = status_map.get(&s.id);
			FacilityServerStatus {
				id: s.id,
				name: s.name.clone().unwrap_or_default(),
				up: st.map(|s| s.short_status()).unwrap_or_default(),
				health: st.map(|s| s.health_state()).unwrap_or_default(),
				rank: s.rank,
				kind: s.kind,
			}
		})
		.collect();

	Ok(Json(ServerGroupCard {
		id: group.id,
		name: group.name,
		notes: group.notes,
		version: rep_status.and_then(|s| s.version.clone()),
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
	pub healthy: bool,
	pub health: serde_json::Value,
	pub extra: serde_json::Value,
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
	let mut conn = state.db.get().await?;
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
	// nodejs lives on the *latest* device connection rather than a
	// contemporary one — the device-side connection metadata isn't
	// versioned in lockstep with status pushes, and looking up "as
	// of" would mostly mislead.
	let nodejs = if let Some(dev_id) = status.device_id {
		DeviceConnection::get_latest_from_device_ids(&mut conn, [dev_id].into_iter())
			.await?
			.into_iter()
			.next()
			.and_then(|d| d.nodejs_version())
	} else {
		None
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
		health: status.health,
		extra: status.extra,
	})))
}

// Touch `Server` so the unused-import warning doesn't fire when this module
// is compiled standalone in some configurations.
#[allow(dead_code)]
fn _server_touch(_s: Server) {}
