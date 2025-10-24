use std::collections::BTreeSet;

use commons_errors::Result;
use commons_versions::VersionStr;
use leptos::serde_json::Value as JsonValue;
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
pub struct ServerDetailData {
	pub server: ServerDetailsData,
	pub device_info: Option<super::devices::DeviceInfo>,
	pub last_status: Option<ServerLastStatusData>,
	pub up: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerLastStatusData {
	pub id: String,
	pub created_at: String,
	pub version: Option<String>,
	pub platform: Option<String>,
	pub postgres: Option<String>,
	pub nodejs: Option<String>,
	pub timezone: Option<String>,
	pub extra: JsonValue,
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
pub async fn server_detail(server_id: String) -> Result<ServerDetailData> {
	ssr::server_detail(server_id).await
}

#[server]
pub async fn update_server(
	server_id: String,
	name: Option<String>,
	host: Option<String>,
	rank: Option<String>,
	device_id: Option<String>,
) -> Result<ServerDetailsData> {
	ssr::update_server(server_id, name, host, rank, device_id).await
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

	pub async fn server_detail(server_id: String) -> Result<super::ServerDetailData> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		let id = server_id
			.parse::<Uuid>()
			.map_err(|e| AppError::custom(format!("Invalid server ID: {}", e)))?;

		let server = Server::get_by_id(&mut conn, id).await?;
		let device_id = server.device_id;

		let server_details = super::ServerDetailsData {
			id: server.id.to_string(),
			name: server.name.unwrap_or_default(),
			kind: server.kind.to_string(),
			rank: server.rank.map_or("unknown".to_string(), |r| r.to_string()),
			host: server.host.0.to_string(),
		};

		let status = Status::latest_for_server(&mut conn, id).await?;

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

		let last_status = if let Some(st) = status.as_ref() {
			let device = if let Some(device_id) = st.device_id {
				DeviceConnection::get_latest_from_device_ids(&mut conn, [device_id].into_iter())
					.await?
					.into_iter()
					.next()
			} else {
				None
			};

			let platform = st.extra("pgVersion").and_then(|pg| pg.as_str()).map(|pg| {
				if pg.contains("Visual C++") || pg.contains("windows") {
					"Windows"
				} else {
					"Linux"
				}
				.into()
			});

			let postgres = st
				.extra("pgVersion")
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

			Some(super::ServerLastStatusData {
				id: st.id.to_string(),
				created_at: st.created_at.to_rfc3339(),
				version: st.version.as_ref().map(|v| v.to_string()),
				platform,
				postgres,
				nodejs,
				timezone: st
					.extra("timezone")
					.and_then(|s| s.as_str().map(|s| s.to_string())),
				extra: st.extra.clone(),
			})
		} else {
			None
		};

		let device_info = if let Some(device_id) = device_id {
			use database::Device;
			let device_with_info = Device::get_with_info(&mut conn, device_id).await?;
			Some(convert_device_with_info_to_device_info(device_with_info))
		} else {
			None
		};

		Ok(super::ServerDetailData {
			server: server_details,
			device_info,
			last_status,
			up,
		})
	}

	fn convert_device_with_info_to_device_info(
		device_with_info: database::DeviceWithInfo,
	) -> crate::fns::devices::DeviceInfo {
		use folktime::duration::Style;

		fn format_relative_time(datetime: chrono::DateTime<chrono::Utc>) -> String {
			let now = chrono::Utc::now();
			let duration = (now - datetime).to_std().unwrap_or_default();
			let relative = folktime::duration::Duration(duration, Style::OneUnitWhole);
			format!("{} ago", relative)
		}

		fn format_key_as_pem(key_data: &[u8]) -> String {
			use base64::prelude::*;

			let base64_data = BASE64_STANDARD.encode(key_data);
			let mut pem = String::with_capacity(base64_data.len() + 100);

			pem.push_str("-----BEGIN PUBLIC KEY-----\n");

			// Split into 64-character lines
			for chunk in base64_data.as_bytes().chunks(64) {
				pem.push_str(&String::from_utf8_lossy(chunk));
				pem.push('\n');
			}

			pem.push_str("-----END PUBLIC KEY-----");
			pem
		}

		fn format_key_as_hex(key_data: &[u8]) -> String {
			hex::encode(key_data)
				.chars()
				.collect::<Vec<_>>()
				.chunks(2)
				.map(|chunk| chunk.iter().collect::<String>())
				.collect::<Vec<_>>()
				.join(":")
				.to_uppercase()
		}

		crate::fns::devices::DeviceInfo {
			device: crate::fns::devices::DeviceData {
				id: device_with_info.device.id.to_string(),
				created_at: device_with_info.device.created_at.to_rfc3339(),
				created_at_relative: format_relative_time(device_with_info.device.created_at),
				updated_at: device_with_info.device.updated_at.to_rfc3339(),
				updated_at_relative: format_relative_time(device_with_info.device.updated_at),
				role: String::from(device_with_info.device.role),
			},
			keys: device_with_info
				.keys
				.into_iter()
				.map(|key| {
					let pem_data = format_key_as_pem(&key.key_data);
					let hex_data = format_key_as_hex(&key.key_data);

					crate::fns::devices::DeviceKeyInfo {
						id: key.id.to_string(),
						device_id: key.device_id.to_string(),
						name: key.name,
						pem_data,
						hex_data,
						created_at: key.created_at.to_rfc3339(),
					}
				})
				.collect(),
			latest_connection: device_with_info.latest_connection.map(|conn| {
				crate::fns::devices::DeviceConnectionData {
					id: conn.id.to_string(),
					created_at: conn.created_at.to_rfc3339(),
					created_at_relative: format_relative_time(conn.created_at),
					device_id: conn.device_id.to_string(),
					ip: conn.ip.addr().to_string(),
					user_agent: conn.user_agent,
				}
			}),
		}
	}

	pub async fn update_server(
		server_id: String,
		name: Option<String>,
		host: Option<String>,
		rank: Option<String>,
		device_id: Option<String>,
	) -> Result<super::ServerDetailsData> {
		use database::{
			server_rank::ServerRank, servers::PartialServer, servers::Server, url_field::UrlField,
		};

		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let id = server_id
			.parse::<Uuid>()
			.map_err(|e| AppError::custom(format!("Invalid server ID: {}", e)))?;

		let parsed_host = if let Some(host_str) = host {
			Some(UrlField(host_str.parse().map_err(|e| {
				AppError::custom(format!("Invalid URL: {}", e))
			})?))
		} else {
			None
		};

		let parsed_rank = if let Some(rank_str) = rank {
			Some(
				ServerRank::try_from(rank_str)
					.map_err(|_| AppError::custom("Invalid server rank"))?,
			)
		} else {
			None
		};

		let parsed_device_id = if let Some(device_id_str) = device_id {
			if device_id_str.is_empty() {
				Some(None)
			} else {
				Some(Some(device_id_str.parse::<Uuid>().map_err(|e| {
					AppError::custom(format!("Invalid device ID: {}", e))
				})?))
			}
		} else {
			None
		};

		let update_data = PartialServer {
			id,
			name,
			kind: None,
			rank: parsed_rank,
			host: parsed_host,
			device_id: parsed_device_id,
		};

		let server = Server::update(&mut conn, id, update_data).await?;

		Ok(super::ServerDetailsData {
			id: server.id.to_string(),
			name: server.name.unwrap_or_default(),
			kind: server.kind.to_string(),
			rank: server.rank.map_or("unknown".to_string(), |r| r.to_string()),
			host: server.host.0.to_string(),
		})
	}
}
