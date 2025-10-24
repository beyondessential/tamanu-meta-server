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

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use std::collections::BTreeSet;

	use axum::extract::State;
	use chrono::{TimeDelta, Utc};
	use commons_errors::{AppError, Result};
	use database::{Db, devices::DeviceConnection, servers::Server, statuses::Status};
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

		let up = status.as_ref().map_or("gone".into(), |st| {
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
		});

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
}
