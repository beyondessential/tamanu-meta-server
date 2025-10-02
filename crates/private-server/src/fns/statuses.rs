use std::collections::BTreeSet;

use commons_errors::Result;
use commons_versions::VersionStr;
use leptos::server;
use serde::{Deserialize, Serialize};
#[cfg(feature = "ssr")]
pub use ssr::routes;

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

#[server]
pub async fn greeting() -> Result<String> {
	ssr::greeting().await
}

#[server]
pub async fn summary() -> Result<SummaryData> {
	ssr::summary().await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTableRow {
	pub up: String,
	pub server_id: String,
	pub server_name: String,
	pub server_kind: String,
	pub server_rank: String,
	pub server_host: String,
	pub updated_at: Option<String>,
	pub since: Option<String>,
	pub version: Option<String>,
	pub platform: Option<String>,
	pub postgres: Option<String>,
	pub nodejs: Option<String>,
	pub timezone: Option<String>,
}

#[server]
pub async fn table() -> Result<Vec<ServerTableRow>> {
	ssr::table().await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use std::{
		collections::{BTreeSet, HashMap},
		time::Instant,
	};

	use crate::state::AppState;
	use axum::{Extension, Json, Router, extract::State, routing::get};
	use axum_server_timing::ServerTimingExtension;
	use chrono::{TimeDelta, Utc};
	use commons_errors::Result;
	use commons_servers::tailscale_auth::TailscaleUser;
	use database::{
		Db, devices::DeviceConnection, server_rank::ServerRank, servers::Server, statuses::Status,
	};
	use folktime::duration::{Duration as FolktimeDuration, Style};
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;
	use uuid::Uuid;

	#[derive(Debug, Clone, Serialize, Deserialize)]
	pub struct ServerData {
		pub server: database::servers::Server,
		pub device: Option<database::devices::DeviceConnection>,
		pub status: Option<database::statuses::Status>,
		pub up: String,
		pub since: Option<String>,
		pub platform: Option<String>,
		pub postgres: Option<String>,
		pub nodejs: Option<String>,
	}

	pub fn routes() -> Router<AppState> {
		Router::new().route("/status.json", get(data))
	}

	async fn servers_with_status(db: Db, timing: ServerTimingExtension) -> Result<Vec<ServerData>> {
		let start = Instant::now();
		let mut conn = db.get().await?;
		let statuses: HashMap<Uuid, Status> = Status::latest_for_all_servers(&mut conn)
			.await?
			.into_iter()
			.map(|status| (status.server_id, status))
			.collect();
		timing
			.lock()
			.unwrap()
			.record_timing("statuses".to_string(), start.elapsed(), None);

		let start = Instant::now();
		let device_to_server_ids: HashMap<Uuid, Uuid> = statuses
			.values()
			.filter_map(|status| status.device_id.map(|id| (id, status.server_id)))
			.collect();
		let devices: HashMap<Uuid, DeviceConnection> =
			DeviceConnection::get_latest_from_device_ids(
				&mut conn,
				device_to_server_ids.keys().copied(),
			)
			.await?
			.into_iter()
			.filter_map(|device| {
				device_to_server_ids
					.get(&device.device_id)
					.map(|server_id| (*server_id, device))
			})
			.collect();
		timing
			.lock()
			.unwrap()
			.record_timing("devices".to_string(), start.elapsed(), None);

		let start = Instant::now();
		let servers = Server::get_all(&mut conn).await?;
		timing
			.lock()
			.unwrap()
			.record_timing("servers".to_string(), start.elapsed(), None);

		let start = Instant::now();
		let mut entries = Vec::with_capacity(statuses.len());
		for server in servers {
			if server.name.is_none() {
				continue;
			}

			let device = devices.get(&server.id).cloned();
			let status = statuses.get(&server.id).cloned();
			entries.push(ServerData {
				up: status.as_ref().map_or("gone".into(), |st| {
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
				}),
				since: status.as_ref().map(|st| {
					let duration = st.created_at.signed_duration_since(Utc::now()).abs();
					FolktimeDuration(duration.to_std().unwrap_or_default(), Style::OneUnitWhole)
						.to_string()
				}),
				platform: status
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
					}),
				postgres: status
					.as_ref()
					.and_then(|st| st.extra("pgVersion"))
					.and_then(|pg| pg.as_str())
					.and_then(|pg| pg.split_ascii_whitespace().nth(1))
					.map(|vers| vers.trim_end_matches(',').into()),
				nodejs: device
					.as_ref()
					.and_then(|d| d.user_agent.as_ref())
					.and_then(|ua| {
						ua.split_ascii_whitespace()
							.find_map(|p| p.strip_prefix("Node.js/"))
							.map(ToOwned::to_owned)
					}),
				server,
				device,
				status,
			});
		}
		entries.sort_by_key(|s| (s.server.rank, s.server.name.clone()));
		timing
			.lock()
			.unwrap()
			.record_timing("processing".to_string(), start.elapsed(), None);

		Ok(entries)
	}

	pub async fn greeting() -> Result<String> {
		let state = expect_context::<AppState>();
		Ok(
			if let Some(TailscaleUser { name, .. }) = extract_with_state(&state).await? {
				format!("Hi {name}!")
			} else {
				"Kia Ora!".to_string()
			},
		)
	}

	pub async fn summary() -> Result<SummaryData> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let Extension(timing): Extension<ServerTimingExtension> =
			extract_with_state(&state).await?;

		let entries = servers_with_status(db, timing).await?;
		let versions = entries
			.iter()
			.filter_map(|status| {
				if let (Some(version), Some(ServerRank::Production)) = (
					status.status.as_ref().and_then(|s| s.version.clone()),
					status.server.rank,
				) {
					Some(version)
				} else {
					None
				}
			})
			.collect::<BTreeSet<_>>();
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

	pub async fn table() -> Result<Vec<ServerTableRow>> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let Extension(timing): Extension<ServerTimingExtension> =
			extract_with_state(&state).await?;
		let servers = servers_with_status(db, timing).await?;

		Ok(servers
			.into_iter()
			.map(|s| ServerTableRow {
				up: s.up,
				server_id: s.server.id.to_string(),
				server_name: s.server.name.unwrap_or_default(),
				server_kind: s.server.kind.to_string(),
				server_rank: s
					.server
					.rank
					.map_or("unknown".to_string(), |r| r.to_string()),
				server_host: s.server.host.0.to_string(),
				updated_at: s.status.as_ref().map(|s| s.created_at.to_string()),
				since: s.since,
				version: s
					.status
					.as_ref()
					.and_then(|s| s.version.as_ref().map(|v| v.to_string())),
				platform: s.platform,
				postgres: s.postgres,
				nodejs: s.nodejs,
				timezone: s.status.and_then(|s| {
					s.extra("timezone")
						.and_then(|s| s.as_str().map(|s| s.to_string()))
				}),
			})
			.collect())
	}

	pub async fn data(
		State(db): State<Db>,
		Extension(timing): Extension<ServerTimingExtension>,
	) -> Result<Json<Vec<ServerData>>> {
		Ok(Json(servers_with_status(db, timing).await?))
	}
}
