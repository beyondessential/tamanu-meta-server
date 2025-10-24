use std::collections::BTreeSet;

use commons_errors::Result;
use commons_versions::VersionStr;
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
pub struct FacilityServerCardData {
	pub id: String,
	pub name: String,
	pub up: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentralServerCardData {
	pub id: String,
	pub name: String,
	pub rank: String,
	pub host: String,
	pub up: String,
	pub version: Option<String>,
	pub version_distance: Option<i32>,
	pub facility_servers: Vec<FacilityServerCardData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedServersData {
	pub production: Vec<CentralServerCardData>,
	pub clone: Vec<CentralServerCardData>,
	pub demo: Vec<CentralServerCardData>,
	pub test: Vec<CentralServerCardData>,
	pub dev: Vec<CentralServerCardData>,
}

#[server]
pub async fn summary() -> Result<SummaryData> {
	ssr::summary().await
}

#[server]
pub async fn server_ids() -> Result<Vec<String>> {
	ssr::server_ids().await
}

#[server]
pub async fn server_details(server_id: String) -> Result<ServerDetailsData> {
	ssr::server_details(server_id).await
}

#[server]
pub async fn server_status(server_id: String) -> Result<ServerStatusData> {
	ssr::server_status(server_id).await
}

#[server]
pub async fn grouped_central_servers() -> Result<GroupedServersData> {
	ssr::grouped_central_servers().await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use std::collections::BTreeSet;

	use axum::extract::State;
	use chrono::{TimeDelta, Utc};
	use commons_errors::{AppError, Result};
	use database::{
		Db, devices::DeviceConnection, server_kind::ServerKind, servers::Server, statuses::Status,
		versions::Version,
	};
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;
	use uuid::Uuid;

	use crate::state::AppState;

	fn calculate_up_status(status: Option<&Status>) -> String {
		status.map_or("gone".into(), |st| {
			let since = st.created_at.signed_duration_since(Utc::now()).abs();
			if since > TimeDelta::minutes(30) {
				"down"
			} else if since > TimeDelta::minutes(10) {
				"away"
			} else if since > TimeDelta::minutes(2) {
				"blip"
			} else {
				"up"
			}
			.into()
		})
	}

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

	pub async fn server_ids() -> Result<Vec<String>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		let mut servers = Server::get_all(&mut conn).await?;

		servers.retain(|s| s.name.is_some());
		servers.sort_by_key(|s| (s.rank, s.name.clone()));

		Ok(servers.into_iter().map(|s| s.id.to_string()).collect())
	}

	pub async fn server_details(server_id: String) -> Result<super::ServerDetailsData> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		let id = server_id
			.parse::<Uuid>()
			.map_err(|e| AppError::custom(format!("Invalid server ID: {}", e)))?;
		let server = Server::get_by_id(&mut conn, id).await?;

		Ok(super::ServerDetailsData {
			id: server.id.to_string(),
			name: server.name.unwrap_or_default(),
			kind: server.kind.to_string(),
			rank: server.rank.map_or("unknown".to_string(), |r| r.to_string()),
			host: server.host.0.to_string(),
		})
	}

	pub async fn server_status(server_id: String) -> Result<super::ServerStatusData> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		let id = server_id
			.parse::<Uuid>()
			.map_err(|e| AppError::custom(format!("Invalid server ID: {}", e)))?;

		let status = Status::latest_for_server(&mut conn, id).await?;

		let device_id = status.as_ref().and_then(|s| s.device_id);
		let device = if let Some(device_id) = device_id {
			let devices =
				DeviceConnection::get_latest_from_device_ids(&mut conn, [device_id].into_iter())
					.await?;
			devices.into_iter().next()
		} else {
			None
		};

		let up = calculate_up_status(status.as_ref());

		let platform = status
			.as_ref()
			.and_then(|st| st.extra("pgVersion"))
			.and_then(|pg| pg.as_str())
			.map(|pg| {
				if pg.contains("Visual C++") || pg.contains("windows") {
					"Windows"
				} else {
					"Linux"
				}
				.into()
			});

		let postgres = status
			.as_ref()
			.and_then(|st| st.extra("pgVersion"))
			.and_then(|pg| pg.as_str())
			.and_then(|pg| pg.split_ascii_whitespace().nth(1))
			.map(|vers| vers.trim_end_matches(',').into());

		let nodejs = device
			.as_ref()
			.and_then(|d| d.user_agent.as_ref())
			.and_then(|ua| {
				ua.split_ascii_whitespace()
					.find_map(|p| p.strip_prefix("Node.js/"))
					.map(ToOwned::to_owned)
			});

		Ok(super::ServerStatusData {
			up,
			updated_at: status.as_ref().map(|s| s.created_at.to_rfc3339()),
			version: status
				.as_ref()
				.and_then(|s| s.version.as_ref().map(|v| v.to_string())),
			platform,
			postgres,
			nodejs,
			timezone: status.and_then(|s| {
				s.extra("timezone")
					.and_then(|s| s.as_str().map(|s| s.to_string()))
			}),
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

		// Get all published versions once for version distance calculation
		let all_versions = Version::get_all(&mut conn).await.ok();
		let published_versions: Option<Vec<_>> =
			all_versions.map(|versions| versions.into_iter().filter(|ver| ver.published).collect());

		// Process each central server
		let mut cards: Vec<super::CentralServerCardData> = Vec::new();

		for central in central_servers {
			let central_status = status_map.get(&central.id);
			let up = calculate_up_status(central_status.copied());

			// Calculate version distance
			let version_distance =
				central_status
					.and_then(|st| st.version.as_ref())
					.and_then(|v| {
						published_versions.as_ref().and_then(|versions| {
							if versions.is_empty() {
								return None;
							}

							let latest = versions.first()?;
							let latest_semver = latest.as_semver();
							let current_semver = &v.0;

							// Calculate distance: 1000 + minor diff if major differs, else just minor diff
							let distance = if latest_semver.major != current_semver.major {
								1000 + (latest_semver.minor as i32 - current_semver.minor as i32)
									.abs()
							} else {
								(latest_semver.minor as i32 - current_semver.minor as i32).abs()
							};

							Some(distance)
						})
					});

			// Get facility servers for this central
			let facility_servers: Vec<super::FacilityServerCardData> = servers
				.iter()
				.filter(|s| s.parent_server_id == Some(central.id))
				.map(|facility| {
					let facility_status = status_map.get(&facility.id);
					let facility_up = calculate_up_status(facility_status.copied());
					super::FacilityServerCardData {
						id: facility.id.to_string(),
						name: facility.name.clone().unwrap_or_default(),
						up: facility_up,
					}
				})
				.collect();

			cards.push(super::CentralServerCardData {
				id: central.id.to_string(),
				name: central.name.clone().unwrap_or_default(),
				rank: central
					.rank
					.map_or("unknown".to_string(), |r| r.to_string()),
				host: central.host.0.to_string(),
				up,
				version: central_status.and_then(|s| s.version.as_ref().map(|v| v.to_string())),
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
			match card.rank.as_str() {
				"production" => production.push(card),
				"clone" => clone.push(card),
				"demo" => demo.push(card),
				"test" => test.push(card),
				"dev" => dev.push(card),
				_ => {} // Skip unknown ranks
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
