use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{server::rank::ServerRank, status::ShortStatus, version::VersionStr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacilityServerStatus {
	pub id: Uuid,
	pub name: String,
	pub up: ShortStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentralServerCard {
	pub id: Uuid,
	pub name: String,
	pub rank: Option<ServerRank>,
	pub host: String,
	pub up: ShortStatus,
	pub version: Option<VersionStr>,
	pub version_distance: Option<u64>,
	pub facility_servers: Vec<FacilityServerStatus>,
}
