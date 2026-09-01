use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
	server::{app_type::ApplicationType, rank::ServerRank},
	status::{HealthState, OperatorPresence, ShortStatus},
	version::VersionStr,
};

/// Current status of a single server within a group, as shown on the status
/// dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FacilityServerStatus {
	/// Unique identifier of the server.
	pub id: Uuid,
	/// Name of the server.
	pub name: String,
	/// Reachability of the server, based on how recently it last reported a
	/// status update.
	pub up: ShortStatus,
	/// The server's self-reported health.
	pub health: HealthState,
	/// Whether canopy alerts on this server's checks. An unmonitored
	/// server's reachability and health are determined and presented as
	/// normal, but raise nothing — so consumers mark it, rather than
	/// showing a failure that nobody is being paged about.
	// spec: CHK#monitoring-gate
	pub is_monitored: bool,
	/// People currently connected to this server, from its latest status
	/// update. Always empty unless the server is actively reporting (`up`
	/// or `blip`) — a stale report can't be trusted to reflect who is
	/// connected right now.
	pub operators: Vec<OperatorPresence>,
	/// The server's rank, when set. Lets consumers group or order servers by
	/// rank (e.g. production before clone, demo, and so on) in a consistent
	/// order.
	pub rank: Option<ServerRank>,
	/// The application the server runs, presented alongside its role.
	// spec: APP
	pub r#type: ApplicationType,
}

/// A status-dashboard card summarising one group of equivalent servers, with
/// a status entry per member. The group's version is taken from whichever
/// member reported most recently.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerGroupCard {
	/// Unique identifier of the group.
	pub id: Uuid,
	/// Name of the group.
	pub name: String,
	/// Free-text notes about the group.
	pub notes: String,
	/// Version reported by the most recently-reporting member of the group,
	/// when known.
	pub version: Option<VersionStr>,
	/// How far the reported `version` lags behind the latest known release,
	/// when both are known. Combines the major and minor version gaps into a
	/// single number that grows the further behind the version is (major
	/// gaps count for more than minor ones); `0` means the version is
	/// current or newer than the latest known release.
	pub version_distance: Option<u64>,
	/// Status of each server belonging to this group.
	pub members: Vec<FacilityServerStatus>,
}
