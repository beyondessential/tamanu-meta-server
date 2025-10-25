use std::collections::{BTreeMap, BTreeSet};

use commons_errors::Result;
use commons_types::{server::cards::CentralServerCard, version::VersionStr};
use leptos::server;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedServersData {
	pub production: Vec<CentralServerCard>,
	pub clone: Vec<CentralServerCard>,
	pub demo: Vec<CentralServerCard>,
	pub test: Vec<CentralServerCard>,
	pub dev: Vec<CentralServerCard>,
}

#[server]
pub async fn summary() -> Result<SummaryData> {
	ssr::summary().await
}

#[server]
pub async fn server_grouped_ids() -> Result<BTreeMap<String, Vec<String>>> {
	ssr::server_grouped_ids().await
}

#[server]
pub async fn server_details(server_id: String) -> Result<CentralServerCard> {
	ssr::server_details(server_id).await
}

#[server]
pub async fn grouped_central_servers() -> Result<GroupedServersData> {
	ssr::grouped_central_servers().await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use std::collections::{BTreeMap, BTreeSet, HashMap};

	use axum::extract::State;
	use commons_errors::{AppError, Result};
	use commons_types::{
		server::{cards::FacilityServerStatus, kind::ServerKind, rank::ServerRank},
		version::VersionStr,
	};
	use database::{Db, servers::Server, statuses::Status, versions::Version};
	use itertools::Itertools;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;
	use uuid::Uuid;

	use crate::state::AppState;

	pub async fn summary() -> Result<SummaryData> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

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

		Ok(SummaryData {
			bracket,
			releases,
			versions,
		})
	}

	pub async fn server_grouped_ids() -> Result<BTreeMap<String, Vec<String>>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		let servers = Server::get_all(&mut conn).await?;

		let groups = servers
			.into_iter()
			.filter(|s| s.name.is_some() && s.kind == ServerKind::Central && s.rank.is_some())
			.sorted_by_key(|s| s.rank)
			.chunk_by(|s| s.rank.unwrap());

		Ok(groups
			.into_iter()
			.map(|(rank, group)| {
				(
					rank.to_string(),
					group
						.sorted_by_key(|s| s.name.clone().unwrap())
						.map(|s| s.id.to_string())
						.collect(),
				)
			})
			.collect())
	}

	pub async fn server_details(server_id: String) -> Result<super::CentralServerCard> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		let id = server_id
			.parse::<Uuid>()
			.map_err(|e| AppError::custom(format!("Invalid server ID: {}", e)))?;
		let central = Server::get_by_id(&mut conn, id).await?;

		let latest_version = Version::get_latest_matching(&mut conn, "*".parse()?)
			.await?
			.as_semver();

		let central_status = Status::latest_for_server(&mut conn, id).await?;
		let central_up = central_status
			.as_ref()
			.map(|s| s.short_status())
			.unwrap_or_default();
		let version_distance = central_status
			.as_ref()
			.map(|s| s.distance_from_version(&latest_version))
			.flatten();

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

		Ok(super::CentralServerCard {
			id: central.id,
			name: central.name.unwrap_or_default(),
			rank: central.rank,
			host: central.host.0.to_string(),
			up: central_up,
			version: central_status.map(|s| s.version).flatten(),
			version_distance,
			facility_servers,
		})
	}

	pub async fn grouped_central_servers() -> Result<super::GroupedServersData> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		// Get all servers
		let servers = Server::get_all(&mut conn).await?;

		// Separate central and facility servers
		let central_servers: Vec<_> = servers
			.iter()
			.filter(|s| s.kind == ServerKind::Central && s.name.is_some())
			.collect();

		// Get latest statuses for all servers in one query
		let server_ids: Vec<Uuid> = servers.iter().map(|s| s.id).collect();
		let statuses = Status::latest_for_servers(&mut conn, &server_ids).await?;

		// Build a map of server_id -> status
		let status_map: std::collections::HashMap<Uuid, &Status> =
			statuses.iter().map(|s| (s.server_id, s)).collect();

		let latest_version = Version::get_latest_matching(&mut conn, "*".parse()?)
			.await?
			.as_semver();

		// Process each central server
		let mut cards: Vec<super::CentralServerCard> = Vec::new();

		for central in central_servers {
			let central_status = status_map.get(&central.id);
			let up = central_status.map(|s| s.short_status()).unwrap_or_default();
			let version_distance = central_status
				.map(|s| s.distance_from_version(&latest_version))
				.flatten();

			// Get facility servers for this central
			let facility_servers: Vec<FacilityServerStatus> = servers
				.iter()
				.filter(|s| s.parent_server_id == Some(central.id))
				.map(|facility| {
					let facility_status = status_map.get(&facility.id);
					FacilityServerStatus {
						id: facility.id,
						name: facility.name.clone().unwrap_or_default(),
						up: facility_status
							.map(|s| s.short_status())
							.unwrap_or_default(),
					}
				})
				.collect();

			cards.push(CentralServerCard {
				id: central.id,
				name: central.name.clone().unwrap_or_default(),
				rank: central.rank,
				host: central.host.0.to_string(),
				up,
				version: central_status.and_then(|s| s.version.clone()),
				version_distance,
				facility_servers,
			});
		}

		// Group by rank
		let mut production = Vec::new();
		let mut clone = Vec::new();
		let mut demo = Vec::new();
		let mut test = Vec::new();
		let mut dev = Vec::new();

		for card in cards {
			match card.rank.unwrap_or_default() {
				ServerRank::Production => production.push(card),
				ServerRank::Clone => clone.push(card),
				ServerRank::Demo => demo.push(card),
				ServerRank::Test => test.push(card),
				ServerRank::Dev => dev.push(card),
			}
		}

		Ok(super::GroupedServersData {
			production,
			clone,
			demo,
			test,
			dev,
		})
	}
}
