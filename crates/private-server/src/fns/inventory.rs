//! Operator-facing environment inventory: a group's live servers at one rank,
//! the address each is reached at, and the variables that configure them.
//!
//! Assembled from what Canopy already holds — group membership, rank, product
//! and kind, the bound device's tailnet name, and the server/group tag merge —
//! so configuration tooling reads the fleet from here rather than from a file
//! kept in step by hand.
// spec: INV

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use commons_types::{
	Uuid,
	server::{RESERVED_TAG_PREFIX, TagMap, kind::ServerKind, product::Product, rank::ServerRank},
};
use database::{Device, server_groups::ServerGroup, servers::Server};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(for_group))
}

/// Which environment to serve the inventory for: exactly one of the group's
/// identifier or its name, and the rank where the group holds more than one
/// environment.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct InventoryArgs {
	/// Identifier of the server group.
	#[serde(default)]
	pub server_group_id: Option<Uuid>,
	/// Name of the server group, matched exactly.
	#[serde(default)]
	pub group: Option<String>,
	/// Rank of the environment within the group. Required only where the
	/// group's live servers span more than one rank.
	#[serde(default)]
	pub rank: Option<ServerRank>,
}

/// One server in an environment.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct InventoryHost {
	/// Identifier of the server.
	pub id: Uuid,
	/// The server's name within its group, falling back to its host and then
	/// its identifier, so a member always has something to be addressed as.
	pub name: String,
	/// The application this server runs.
	pub product: Product,
	/// The server's role within its product's topology.
	pub kind: ServerKind,
	/// The server's environment tier, where one is set.
	pub rank: Option<ServerRank>,
	/// The address to reach the host at: its bound device's tailnet name, or
	/// its recorded host where no device is bound. Null when Canopy holds
	/// neither, in which case a variable has to supply it.
	pub address: Option<String>,
	/// The host's variables: its own tags over its group's, with the reserved
	/// read-only tags left out.
	#[schema(value_type = Object)]
	pub vars: BTreeMap<String, Value>,
}

/// An environment's inventory: its servers and the variables that configure
/// them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct InventoryView {
	/// Identifier of the server group the inventory covers.
	pub group_id: Uuid,
	/// Name of the server group.
	pub group: String,
	/// Rank of the environment served, where its servers carry one.
	pub rank: Option<ServerRank>,
	/// Variables belonging to the group rather than to any one server.
	/// Every server carries these too, under its own overrides.
	#[schema(value_type = Object)]
	pub vars: BTreeMap<String, Value>,
	/// The group's live members, ordered by name.
	pub hosts: Vec<InventoryHost>,
}

/// A stored tag value as a variable.
///
/// `true` and `false` become booleans and a JSON array or object becomes that
/// array or object; everything else stays the text it was stored as, a bare
/// number included, since a number here is far more often a version or an
/// identifier than a quantity.
fn decode(value: &str) -> Value {
	match value {
		"true" => Value::Bool(true),
		"false" => Value::Bool(false),
		_ if value.starts_with('[') || value.starts_with('{') => {
			serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned()))
		}
		_ => Value::String(value.to_owned()),
	}
}

fn vars(tags: &TagMap) -> BTreeMap<String, Value> {
	tags.0
		.iter()
		.filter(|(key, _)| !key.starts_with(RESERVED_TAG_PREFIX))
		.map(|(key, value)| (key.clone(), decode(value)))
		.collect()
}

async fn resolve_group(
	conn: &mut database::diesel_async::AsyncPgConnection,
	args: InventoryArgs,
) -> Result<ServerGroup> {
	match (args.server_group_id, args.group) {
		(Some(id), None) => ServerGroup::get_by_id(conn, id).await,
		(None, Some(name)) => {
			let (live, archived): (Vec<_>, Vec<_>) = ServerGroup::find_by_name(conn, &name)
				.await?
				.into_iter()
				.partition(|group| group.deleted_at.is_none());
			if live.len() > 1 {
				return Err(AppError::Conflict(format!(
					"{name:?} names {} server groups; ask by identifier",
					live.len()
				)));
			}
			live.into_iter()
				.next()
				.or_else(|| archived.into_iter().next())
				.ok_or_else(|| AppError::NotFound(format!("no server group named {name:?}")))
		}
		_ => Err(AppError::BadRequest(
			"give exactly one of server_group_id or group".into(),
		)),
	}
}

/// Serve one environment's inventory.
///
/// Refuses a group Canopy does not have, one that has been archived, one
/// holding several environments with no rank named, and a rank with no live
/// server to configure, saying which it was: a refusal is a decision to
/// respect, and a caller has to be able to tell it from Canopy being
/// unreachable.
#[utoipa::path(
	post,
	path = "/for_group",
	operation_id = "inventory_for_group",
	tag = "inventory",
	security(("tailscale-user" = [])),
	request_body = InventoryArgs,
	responses(
		(status = 200, body = InventoryView),
		(status = 404, description = "No such server group", body = ProblemDetailsSchema),
		(status = 409, description = "Archived, empty, or ambiguously named", body = ProblemDetailsSchema),
	),
)]
pub async fn for_group(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<InventoryArgs>,
) -> Result<Json<InventoryView>> {
	let mut conn = state.db.get().await?;
	let args_rank = args.rank;
	let group = resolve_group(&mut conn, args).await?;

	if group.deleted_at.is_some() {
		return Err(AppError::Conflict(format!(
			"server group {:?} is archived",
			group.name
		)));
	}

	let members = Server::list_live_in_group(&mut conn, group.id).await?;
	if members.is_empty() {
		return Err(AppError::Conflict(format!(
			"server group {:?} has no live members",
			group.name
		)));
	}

	let ranks: BTreeSet<Option<ServerRank>> = members.iter().map(|server| server.rank).collect();
	let rank = match args_rank {
		Some(rank) => Some(rank),
		None if ranks.len() > 1 => {
			return Err(AppError::Conflict(format!(
				"server group {:?} holds {} environments ({}); name the rank",
				group.name,
				ranks.len(),
				ranks
					.iter()
					.map(|rank| rank.map_or("unranked".to_string(), |rank| rank.to_string()))
					.collect::<Vec<_>>()
					.join(", ")
			)));
		}
		None => ranks.into_iter().next().flatten(),
	};

	let servers: Vec<Server> = members
		.into_iter()
		.filter(|server| server.rank == rank)
		.collect();
	if servers.is_empty() {
		return Err(AppError::Conflict(format!(
			"server group {:?} has no live server at rank {}",
			group.name,
			rank.map_or("unranked".to_string(), |rank| rank.to_string())
		)));
	}

	let device_ids: Vec<Uuid> = servers
		.iter()
		.filter_map(|server| server.device_id)
		.collect();
	let tailnet = Device::tailscale_names_by_ids(&mut conn, &device_ids).await?;

	let hosts = servers
		.into_iter()
		.map(|server| {
			let host = server
				.host
				.as_ref()
				.and_then(|host| host.0.host_str().map(str::to_owned));
			let address = server
				.device_id
				.and_then(|device| tailnet.get(&device).cloned())
				.or_else(|| host.clone());
			InventoryHost {
				name: server
					.name
					.clone()
					.or(host)
					.unwrap_or_else(|| server.id.to_string()),
				vars: vars(&server.tags.merged_with(&group.tags)),
				id: server.id,
				product: server.product,
				kind: server.kind,
				rank: server.rank,
				address,
			}
		})
		.collect();

	Ok(Json(InventoryView {
		group_id: group.id,
		group: group.name,
		rank,
		vars: vars(&group.tags),
		hosts,
	}))
}
