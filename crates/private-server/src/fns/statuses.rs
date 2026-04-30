use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::Result;
use commons_types::{
	server::{
		cards::{CentralServerCard, FacilityServerStatus},
		kind::ServerKind,
		rank::ServerRank,
	},
	version::VersionStr,
};
use database::{servers::Server, statuses::Status, versions::Version};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct LiveVersionsBracket {
	pub min: VersionStr,
	pub max: VersionStr,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummaryData {
	pub bracket: LiveVersionsBracket,
	pub releases: BTreeSet<(u64, u64)>,
	pub versions: BTreeSet<VersionStr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDetailsData {
	pub id: String,
	pub name: String,
	pub kind: String,
	pub rank: String,
	pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatusData {
	pub up: String,
	pub updated_at: Option<String>,
	pub version: Option<String>,
	pub platform: Option<String>,
	pub postgres: Option<String>,
	pub nodejs: Option<String>,
	pub timezone: Option<String>,
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/summary", post(summary))
		.route("/server_grouped_ids", post(server_grouped_ids))
		.route("/server_details", post(server_details))
}

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

pub async fn server_grouped_ids(
	State(state): State<AppState>,
) -> Result<Json<BTreeMap<ServerRank, Vec<Uuid>>>> {
	let mut conn = state.db.get().await?;
	let servers = Server::list_by_kind(&mut conn, ServerKind::Central, 0, None).await?;

	let groups = servers
		.into_iter()
		.filter(|s| s.name.is_some() && s.rank.is_some())
		.sorted_by_key(|s| s.rank)
		.chunk_by(|s| s.rank.unwrap());

	let map: BTreeMap<ServerRank, Vec<Uuid>> = groups
		.into_iter()
		.map(|(rank, group)| {
			(
				rank,
				group
					.sorted_by_key(|s| s.name.clone().unwrap())
					.map(|s| s.id)
					.collect(),
			)
		})
		.collect();
	Ok(Json(map))
}

#[derive(Deserialize)]
pub struct ServerDetailsArgs {
	pub server_id: Uuid,
}

pub async fn server_details(
	State(state): State<AppState>,
	Json(args): Json<ServerDetailsArgs>,
) -> Result<Json<CentralServerCard>> {
	let mut conn = state.db.get().await?;

	let central = Server::get_by_id(&mut conn, args.server_id).await?;

	let latest_version = Version::get_latest_matching(&mut conn, "*".parse()?)
		.await?
		.as_semver();

	let central_status = Status::latest_for_server(&mut conn, args.server_id).await?;
	let central_up = central_status
		.as_ref()
		.map(|s| s.short_status())
		.unwrap_or_default();
	let version_distance = central_status
		.as_ref()
		.and_then(|s| s.distance_from_version(&latest_version));

	let facilities = central.get_children(&mut conn).await?;
	let facility_ids = facilities.iter().map(|f| f.id).collect::<Vec<_>>();
	let facility_statuses = Status::latest_for_servers(&mut conn, &facility_ids)
		.await?
		.into_iter()
		.map(|s| (s.server_id, s))
		.collect::<HashMap<_, _>>();
	let facility_servers = facilities
		.into_iter()
		.map(|f| {
			let facility_status = facility_statuses.get(&f.id);
			FacilityServerStatus {
				id: f.id,
				name: f.name.clone().unwrap_or_default(),
				up: facility_status
					.map(|s| s.short_status())
					.unwrap_or_default(),
			}
		})
		.collect();

	Ok(Json(CentralServerCard {
		id: central.id,
		name: central.name.unwrap_or_default(),
		rank: central.rank,
		host: central.host.0.to_string(),
		up: central_up,
		version: central_status.and_then(|s| s.version),
		version_distance,
		facility_servers,
	}))
}
