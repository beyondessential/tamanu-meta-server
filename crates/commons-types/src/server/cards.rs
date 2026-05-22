use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
	status::{HealthState, ShortStatus},
	version::VersionStr,
};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FacilityServerStatus {
	pub id: Uuid,
	pub name: String,
	pub up: ShortStatus,
	pub health: HealthState,
}

/// Status-page card for a server group. Replaces the old per-central-server
/// card: each card now stands for a group of equal-level servers (no implicit
/// root). The card carries the headline name plus the per-member status dots
/// — version comes from the most recently-pushing member.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerGroupCard {
	pub id: Uuid,
	pub name: String,
	pub notes: String,
	pub version: Option<VersionStr>,
	pub version_distance: Option<u64>,
	pub members: Vec<FacilityServerStatus>,
}
