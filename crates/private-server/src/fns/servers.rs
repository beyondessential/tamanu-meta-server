use commons_errors::Result;
use leptos::serde_json::Value as JsonValue;
use leptos::server;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDetailsData {
	pub id: String,
	pub name: String,
	pub kind: String,
	pub rank: String,
	pub host: String,
	pub parent_server_id: Option<String>,
	pub parent_server_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerListItem {
	pub id: String,
	pub name: Option<String>,
	pub kind: String,
	pub rank: Option<String>,
	pub host: String,
	pub parent_server_id: Option<String>,
	pub parent_server_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDetailData {
	pub server: ServerDetailsData,
	pub device_info: Option<super::devices::DeviceInfo>,
	pub last_status: Option<ServerLastStatusData>,
	pub up: String,
	pub child_servers: Vec<ChildServerData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildServerData {
	pub id: String,
	pub name: String,
	pub kind: String,
	pub rank: String,
	pub host: String,
	pub up: String,
	pub last_status: Option<ServerLastStatusData>,
	pub device_info: Option<super::devices::DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerLastStatusData {
	pub id: String,
	pub created_at: String,
	pub version: Option<String>,
	pub version_distance: Option<i32>,
	pub platform: Option<String>,
	pub postgres: Option<String>,
	pub nodejs: Option<String>,
	pub timezone: Option<String>,
	pub extra: JsonValue,
}

#[server]
pub async fn list_all_servers() -> Result<Vec<ServerListItem>> {
	ssr::list_all_servers().await
}

#[server]
pub async fn list_central_servers() -> Result<Vec<ServerListItem>> {
	ssr::list_central_servers().await
}

#[server]
pub async fn list_facility_servers() -> Result<Vec<ServerListItem>> {
	ssr::list_facility_servers().await
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
	parent_id: Option<String>,
) -> Result<ServerDetailsData> {
	ssr::update_server(server_id, name, host, rank, device_id, parent_id).await
}

#[server]
pub async fn search_central_servers(query: String) -> Result<Vec<ServerListItem>> {
	ssr::search_central_servers(query).await
}

#[server]
pub async fn assign_parent_server(
	server_id: String,
	parent_server_id: String,
) -> Result<ServerDetailsData> {
	ssr::assign_parent_server(server_id, parent_server_id).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use axum::extract::State;
	use chrono::{TimeDelta, Utc};
	use commons_errors::{AppError, Result};

	use database::{
		Db, Device, devices::DeviceConnection, server_rank::ServerRank, servers::PartialServer,
		servers::Server, statuses::Status, url_field::UrlField, versions::Version,
	};
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;
	use uuid::Uuid;

	use crate::state::AppState;

	pub async fn list_all_servers() -> Result<Vec<super::ServerListItem>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let servers = Server::get_all(&mut conn).await?;

		Ok(servers
			.into_iter()
			.map(|s| super::ServerListItem {
				id: s.id.to_string(),
				name: s.name,
				kind: s.kind.to_string(),
				rank: s.rank.map(|r| r.to_string()),
				host: s.host.0.to_string(),
				parent_server_id: s.parent_server_id.map(|id| id.to_string()),
				parent_server_name: None,
			})
			.collect())
	}

	pub async fn list_central_servers() -> Result<Vec<super::ServerListItem>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let servers = Server::get_all(&mut conn).await?;

		let mut centrals: Vec<_> = servers
			.into_iter()
			.filter(|s| s.kind.to_string() == "central")
			.collect();

		// Sort: unnamed first, then by name
		centrals.sort_by(|a, b| match (&a.name, &b.name) {
			(None, None) => std::cmp::Ordering::Equal,
			(None, Some(_)) => std::cmp::Ordering::Less,
			(Some(_), None) => std::cmp::Ordering::Greater,
			(Some(a_name), Some(b_name)) => a_name.cmp(b_name),
		});

		Ok(centrals
			.into_iter()
			.map(|s| super::ServerListItem {
				id: s.id.to_string(),
				name: s.name,
				kind: s.kind.to_string(),
				rank: s.rank.map(|r| r.to_string()),
				host: s.host.0.to_string(),
				parent_server_id: s.parent_server_id.map(|id| id.to_string()),
				parent_server_name: None,
			})
			.collect())
	}

	pub async fn list_facility_servers() -> Result<Vec<super::ServerListItem>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let servers = Server::get_all(&mut conn).await?;

		let mut facilities: Vec<_> = servers
			.into_iter()
			.filter(|s| s.kind.to_string() != "central")
			.collect();

		// Sort: unaffiliated first, then unnamed, then by name
		facilities.sort_by(|a, b| match (&a.parent_server_id, &b.parent_server_id) {
			(None, None) => match (&a.name, &b.name) {
				(None, None) => std::cmp::Ordering::Equal,
				(None, Some(_)) => std::cmp::Ordering::Less,
				(Some(_), None) => std::cmp::Ordering::Greater,
				(Some(a_name), Some(b_name)) => a_name.cmp(b_name),
			},
			(None, Some(_)) => std::cmp::Ordering::Less,
			(Some(_), None) => std::cmp::Ordering::Greater,
			(Some(_), Some(_)) => match (&a.name, &b.name) {
				(None, None) => std::cmp::Ordering::Equal,
				(None, Some(_)) => std::cmp::Ordering::Less,
				(Some(_), None) => std::cmp::Ordering::Greater,
				(Some(a_name), Some(b_name)) => a_name.cmp(b_name),
			},
		});

		// Get parent server names for facilities
		let mut result = Vec::new();
		for s in facilities {
			let parent_name = if let Some(parent_id) = s.parent_server_id {
				Server::get_by_id(&mut conn, parent_id)
					.await
					.ok()
					.and_then(|parent| parent.name)
			} else {
				None
			};

			result.push(super::ServerListItem {
				id: s.id.to_string(),
				name: s.name,
				kind: s.kind.to_string(),
				rank: s.rank.map(|r| r.to_string()),
				host: s.host.0.to_string(),
				parent_server_id: s.parent_server_id.map(|id| id.to_string()),
				parent_server_name: parent_name,
			});
		}

		Ok(result)
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

		let parent_server_name = if let Some(parent_id) = server.parent_server_id {
			let parent = Server::get_by_id(&mut conn, parent_id).await?;
			parent.name
		} else {
			None
		};

		let server_details = super::ServerDetailsData {
			id: server.id.to_string(),
			name: server.name.clone().unwrap_or_default(),
			kind: server.kind.to_string(),
			rank: server.rank.map_or("unknown".to_string(), |r| r.to_string()),
			host: server.host.0.to_string(),
			parent_server_id: server.parent_server_id.map(|id| id.to_string()),
			parent_server_name,
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

			// Compute version distance
			let version_distance = if let Some(ref v) = st.version {
				// Get all published versions and compute distance from latest
				let all_versions = Version::get_all(&mut conn).await.ok();
				all_versions.and_then(|versions| {
					let published_versions: Vec<_> =
						versions.into_iter().filter(|ver| ver.published).collect();
					if published_versions.is_empty() {
						return None;
					}

					let latest = published_versions.first()?;
					let latest_semver = latest.as_semver();
					let current_semver = &v.0;

					// Calculate distance: 1000 + minor diff if major differs, else just minor diff
					let distance = if latest_semver.major != current_semver.major {
						1000 + (latest_semver.minor as i32 - current_semver.minor as i32).abs()
					} else {
						(latest_semver.minor as i32 - current_semver.minor as i32).abs()
					};

					Some(distance)
				})
			} else {
				None
			};

			Some(super::ServerLastStatusData {
				id: st.id.to_string(),
				created_at: st.created_at.to_rfc3339(),
				version: st.version.as_ref().map(|v| v.to_string()),
				version_distance,
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
			let device_with_info = Device::get_with_info(&mut conn, device_id).await?;
			Some(convert_device_with_info_to_device_info(device_with_info))
		} else {
			None
		};

		let child_servers = if server.kind.to_string() == "central" {
			let children = Server::get_children(&mut conn, id).await?;
			let mut child_data = Vec::new();

			for child in children {
				let child_device_info = if let Some(device_id) = child.device_id {
					let device_with_info = Device::get_with_info(&mut conn, device_id).await?;
					Some(convert_device_with_info_to_device_info(device_with_info))
				} else {
					None
				};

				let child_status = Status::latest_for_server(&mut conn, child.id).await?;
				let child_up = child_status.as_ref().map_or("gone".into(), |st| {
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

				let child_last_status = if let Some(st) = child_status.as_ref() {
					let device = if let Some(device_id) = st.device_id {
						DeviceConnection::get_latest_from_device_ids(
							&mut conn,
							[device_id].into_iter(),
						)
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

					// Compute version distance
					let version_distance = if let Some(ref v) = st.version {
						// Get all published versions and compute distance from latest
						let all_versions = Version::get_all(&mut conn).await.ok();
						all_versions.and_then(|versions| {
							let published_versions: Vec<_> =
								versions.into_iter().filter(|ver| ver.published).collect();
							if published_versions.is_empty() {
								return None;
							}

							let latest = published_versions.first()?;
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
					} else {
						None
					};

					Some(super::ServerLastStatusData {
						id: st.id.to_string(),
						created_at: st.created_at.to_rfc3339(),
						version: st.version.as_ref().map(|v| v.to_string()),
						version_distance,
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

				child_data.push(super::ChildServerData {
					id: child.id.to_string(),
					name: child.name.unwrap_or_default(),
					kind: child.kind.to_string(),
					rank: child.rank.map_or("unknown".to_string(), |r| r.to_string()),
					host: child.host.0.to_string(),
					up: child_up,
					last_status: child_last_status,
					device_info: child_device_info,
				});
			}
			child_data
		} else {
			Vec::new()
		};

		Ok(super::ServerDetailData {
			server: server_details,
			device_info,
			last_status,
			up,
			child_servers,
		})
	}

	pub async fn update_server(
		server_id: String,
		name: Option<String>,
		host: Option<String>,
		rank: Option<String>,
		device_id: Option<String>,
		parent_id: Option<String>,
	) -> Result<super::ServerDetailsData> {
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

		let parsed_parent_id = if let Some(parent_id_str) = parent_id {
			if parent_id_str.is_empty() {
				Some(None)
			} else {
				let parent_uuid = parent_id_str
					.parse::<Uuid>()
					.map_err(|e| AppError::custom(format!("Invalid parent server ID: {}", e)))?;

				// Verify parent is a central server
				let parent = Server::get_by_id(&mut conn, parent_uuid).await?;
				if parent.kind != database::server_kind::ServerKind::Central {
					return Err(AppError::custom("Parent server must be of kind 'central'"));
				}

				Some(Some(parent_uuid))
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
			parent_server_id: parsed_parent_id,
		};

		let server = Server::update(&mut conn, id, update_data).await?;

		let parent_server_name = if let Some(parent_id) = server.parent_server_id {
			let parent = Server::get_by_id(&mut conn, parent_id).await?;
			parent.name
		} else {
			None
		};

		Ok(super::ServerDetailsData {
			id: server.id.to_string(),
			name: server.name.unwrap_or_default(),
			kind: server.kind.to_string(),
			rank: server.rank.map_or("unknown".to_string(), |r| r.to_string()),
			host: server.host.0.to_string(),
			parent_server_id: server.parent_server_id.map(|id| id.to_string()),
			parent_server_name,
		})
	}

	pub async fn search_central_servers(query: String) -> Result<Vec<super::ServerListItem>> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let servers = Server::search_central(&mut conn, &query, 10).await?;

		Ok(servers
			.into_iter()
			.map(|s| super::ServerListItem {
				id: s.id.to_string(),
				name: s.name,
				kind: s.kind.to_string(),
				rank: s.rank.map(|r| r.to_string()),
				host: s.host.0.to_string(),
				parent_server_id: s.parent_server_id.map(|id| id.to_string()),
				parent_server_name: None,
			})
			.collect())
	}

	pub async fn assign_parent_server(
		server_id: String,
		parent_server_id: String,
	) -> Result<super::ServerDetailsData> {
		let db = crate::fns::commons::admin_guard().await?;
		let mut conn = db.get().await?;

		let id = server_id
			.parse::<Uuid>()
			.map_err(|e| AppError::custom(format!("Invalid server ID: {}", e)))?;

		let parent_id = parent_server_id
			.parse::<Uuid>()
			.map_err(|e| AppError::custom(format!("Invalid parent server ID: {}", e)))?;

		// Verify parent is a central server
		let parent = Server::get_by_id(&mut conn, parent_id).await?;
		if parent.kind != database::server_kind::ServerKind::Central {
			return Err(AppError::custom("Parent server must be of kind 'central'"));
		}

		let update_data = PartialServer {
			id,
			name: None,
			kind: None,
			rank: None,
			host: None,
			device_id: None,
			parent_server_id: Some(Some(parent_id)),
		};

		let server = Server::update(&mut conn, id, update_data).await?;

		let parent_server_name = if let Some(parent_id) = server.parent_server_id {
			let parent = Server::get_by_id(&mut conn, parent_id).await?;
			parent.name
		} else {
			None
		};

		Ok(super::ServerDetailsData {
			id: server.id.to_string(),
			name: server.name.unwrap_or_default(),
			kind: server.kind.to_string(),
			rank: server.rank.map_or("unknown".to_string(), |r| r.to_string()),
			host: server.host.0.to_string(),
			parent_server_id: server.parent_server_id.map(|id| id.to_string()),
			parent_server_name,
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
}
