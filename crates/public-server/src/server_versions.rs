use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use axum::{
	Router,
	extract::{Query, State},
	response::{Html, IntoResponse, Response},
	routing::get,
};
use commons_errors::{AppError, Result};
use commons_types::{
	server::{kind::ServerKind, rank::ServerRank},
	status::ShortStatus,
	version::VersionStr,
};
use database::{statuses::Status, versions::Version};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tera::Context;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
struct SecretQuery {
	s: String,
}

#[derive(Debug, Serialize)]
struct ServerVersionInfo {
	id: Uuid,
	name: String,
	host: String,
	version: Option<VersionStr>,
	version_distance: Option<u64>,
	up: ShortStatus,
}

#[derive(Debug, Serialize)]
struct RcEnvironment {
	major: i32,
	minor: i32,
	central: String,
	patient: Option<String>,
	facility_1: Option<String>,
	facility_2: Option<String>,
}

impl RcEnvironment {
	fn new(major: i32, minor: i32) -> Self {
		Self {
			major,
			minor,
			central: format!("https://central.release-{major}-{minor}.cd.tamanu.app/"),
			patient: None, // Will be set during probing
			facility_1: Some(format!(
				"https://facility-1.release-{major}-{minor}.cd.tamanu.app/"
			)),
			facility_2: Some(format!(
				"https://facility-2.release-{major}-{minor}.cd.tamanu.app/"
			)),
		}
	}

	async fn probe_url(url: &str) -> bool {
		let client = reqwest::ClientBuilder::new()
			.timeout(std::time::Duration::from_secs(5))
			.build()
			.unwrap();

		match client.head(url).send().await {
			Ok(response) => response.status().is_success(),
			Err(_) => false,
		}
	}

	async fn probe_and_update(mut self) -> Option<Self> {
		// If central doesn't resolve, omit the entire RC
		if !Self::probe_url(&self.central).await {
			return None;
		}

		// Probe both portal and patient URLs in parallel for patient link
		let portal_url = format!(
			"https://portal.release-{}-{}.cd.tamanu.app/",
			self.major, self.minor
		);
		let patient_url = format!(
			"https://patient.release-{}-{}.cd.tamanu.app/",
			self.major, self.minor
		);

		let (portal_available, patient_available) =
			futures::join!(Self::probe_url(&portal_url), Self::probe_url(&patient_url));

		if portal_available {
			self.patient = Some(portal_url);
		} else if patient_available {
			self.patient = Some(patient_url);
		} else {
			self.patient = None;
		}

		if let Some(ref url) = self.facility_1
			&& !Self::probe_url(url).await
		{
			self.facility_1 = None;
		}

		if let Some(ref url) = self.facility_2
			&& !Self::probe_url(url).await
		{
			self.facility_2 = None;
		}

		Some(self)
	}
}

pub fn routes() -> Router<AppState> {
	Router::new().route("/", get(server_versions_page))
}

async fn server_versions_page(
	Query(query): Query<SecretQuery>,
	State(state): State<crate::state::AppState>,
) -> Result<Response> {
	let Some(secret) = &state.server_versions_secret else {
		return Err(AppError::AuthFailed {
			reason: "Server versions endpoint not configured".to_string(),
		});
	};

	let mut provided_hasher = DefaultHasher::new();
	query.s.hash(&mut provided_hasher);
	let provided_hash = provided_hasher.finish();

	let mut expected_hasher = DefaultHasher::new();
	secret.hash(&mut expected_hasher);
	let expected_hash = expected_hasher.finish();

	let equal: bool = provided_hash
		.to_ne_bytes()
		.ct_eq(&expected_hash.to_ne_bytes())
		.into();

	if !equal {
		return Err(AppError::AuthFailed {
			reason: "Invalid secret".to_string(),
		});
	}

	let db = &state.db;
	let tera = &state.tera;
	let mut conn = db.get().await?;

	let servers = {
		use database::schema::servers::dsl::*;

		servers
			.select((id, name, host))
			.filter(
				rank.eq(ServerRank::Production)
					.and(kind.eq(ServerKind::Central)),
			)
			.order(name.asc())
			.load::<(Uuid, Option<String>, String)>(&mut conn)
			.await?
	};

	let latest_version = Version::get_latest_matching(&mut conn, "*".parse()?)
		.await
		.ok()
		.map(|v| v.as_semver());

	let server_ids: Vec<Uuid> = servers.iter().map(|(id, _, _)| *id).collect();
	let statuses = if !server_ids.is_empty() {
		Status::latest_for_servers(&mut conn, &server_ids).await?
	} else {
		Vec::new()
	};

	let mut server_infos: Vec<ServerVersionInfo> = Vec::new();
	for (id, name, host) in servers {
		let status = statuses.iter().find(|s| s.server_id == id);

		let version = status.and_then(|s| s.version.clone());
		let up = status.map(|s| s.short_status()).unwrap_or_default();

		let version_distance = if let (Some(_), Some(latest)) = (&version, &latest_version) {
			status.and_then(|s| s.distance_from_version(latest))
		} else {
			None
		};

		server_infos.push(ServerVersionInfo {
			id,
			name: name.unwrap_or_else(|| host.clone()),
			host,
			version,
			version_distance,
			up,
		});
	}

	let rc_environments = if let Some(ref latest) = latest_version {
		let current_major = latest.major as i32;
		let current_minor = latest.minor as i32;
		let candidates = vec![
			RcEnvironment::new(current_major, current_minor),
			RcEnvironment::new(current_major, current_minor + 1),
			RcEnvironment::new(current_major, current_minor + 2),
		];

		let futures = candidates.into_iter().map(|env| env.probe_and_update());
		let results = join_all(futures).await;

		results.into_iter().flatten().collect()
	} else {
		Vec::new()
	};

	let mut context = Context::new();
	context.insert("latest_version", &latest_version);
	context.insert("servers", &server_infos);
	context.insert("rc_environments", &rc_environments);

	let html = tera.render("server_versions", &context)?;
	Ok(Html(html).into_response())
}
