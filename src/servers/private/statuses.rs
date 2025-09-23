use std::{
	collections::{BTreeSet, HashMap},
	sync::Arc,
	time::Instant,
};

use axum::{
	Extension, Json,
	extract::State,
	response::Html,
	routing::{Router, get, post},
};
use axum_server_timing::ServerTimingExtension;
use chrono::{TimeDelta, Utc};
use folktime::duration::{Duration as FolktimeDuration, Style};
use serde::Serialize;
use tera::{Context, Tera};
use uuid::Uuid;

use crate::{
	db::{devices::DeviceConnection, server_rank::ServerRank, servers::Server, statuses::Status},
	error::Result,
	servers::{headers::TailscaleUserName, version::VersionStr},
	state::{AppState, Db},
};

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/status", get(view))
		.route("/status.json", get(data))
		.route("/reload", post(reload))
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct LiveVersionsBracket {
	pub min: VersionStr,
	pub max: VersionStr,
}

#[derive(Debug, Clone, Serialize)]
struct ServerData {
	server: Server,
	device: Option<DeviceConnection>,
	status: Option<Status>,
	up: &'static str,
	since: Option<String>,
	platform: Option<&'static str>,
	postgres: Option<String>,
	nodejs: Option<String>,
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
	let devices: HashMap<Uuid, DeviceConnection> = DeviceConnection::get_latest_from_device_ids(
		&mut conn,
		device_to_server_ids.keys().copied(),
	)
	.await?
	.into_iter()
	.filter_map(|device| {
		if let Some(server_id) = device_to_server_ids.get(&device.device_id) {
			Some((*server_id, device))
		} else {
			None
		}
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
			up: status.as_ref().map_or("gone", |st| {
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
				}),
			postgres: status
				.as_ref()
				.and_then(|st| st.extra("pgVersion"))
				.and_then(|pg| pg.as_str())
				.and_then(|pg| pg.split_ascii_whitespace().skip(1).next())
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

async fn view(
	State(db): State<Db>,
	State(tera): State<Arc<Tera>>,
	TailscaleUserName(user_name): TailscaleUserName,
	Extension(timing): Extension<ServerTimingExtension>,
) -> Result<Html<String>> {
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

	let greeting = match user_name {
		Some(name) => format!("Hi {}!", name),
		None => "Kia Ora!".to_string(),
	};

	let mut context = Context::new();
	context.insert("title", "Server statuses");
	context.insert("entries", &entries);
	context.insert("bracket", &bracket);
	context.insert("versions", &versions);
	context.insert("releases", &releases);
	context.insert("greeting", &greeting);
	let html = tera.render("statuses", &context)?;
	Ok(Html(html))
}

async fn data(
	State(db): State<Db>,
	Extension(timing): Extension<ServerTimingExtension>,
) -> Result<Json<Vec<ServerData>>> {
	Ok(Json(servers_with_status(db, timing).await?))
}

async fn reload(State(AppState { db, .. }): State<AppState>) -> Result<()> {
	let mut db = db.get().await?;
	Status::ping_servers_and_save(&mut db).await?;
	Ok(())
}
