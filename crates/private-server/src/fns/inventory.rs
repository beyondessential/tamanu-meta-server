//! Operator-facing environment inventory: a group's live servers at one rank,
//! the address each is reached at, and the variables that configure them.
//!
//! Assembled from what Canopy already holds — group membership, rank, product
//! and kind, the bound device's tailnet name, and the server/group tag merge —
//! so configuration tooling reads the fleet from here rather than from a file
//! kept in step by hand.
//!
//! It carries the secret variables too (see [`super::inventory_secrets`]), which
//! is why it is served to an administrator alone.
// spec: INV

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{backup_secrets::BackupSecrets, tailscale_auth::TailscaleAdmin};
use commons_types::{
	Uuid,
	server::{RESERVED_TAG_PREFIX, TagMap, kind::ServerKind, product::Product, rank::ServerRank},
};
use database::{
	Device,
	inventory_secret_variables::{InventorySecretVariable, SecretScope},
	server_groups::ServerGroup,
	servers::Server,
};
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
	/// The address to reach the server at: its bound device's tailnet name, or
	/// its recorded host where no device is bound. Null when Canopy holds
	/// neither, in which case a variable has to supply it.
	pub address: Option<String>,
	/// The server's effective variables: its own tags over its group's, with
	/// the reserved read-only tags left out. This is what a run acts on.
	pub vars: VarMap,
	/// The variables the server sets itself, so a value inherited from the
	/// group can be told from one set here even where the two agree.
	pub own_vars: VarMap,
	/// Which of `vars` are secret, so a caller can keep them out of anything it
	/// writes down.
	pub secret_vars: Vec<String>,
}

/// An environment's inventory: its servers and the variables that configure
/// them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct InventoryView {
	/// Identifier of the server group the inventory covers.
	pub group_id: Uuid,
	/// Name of the server group.
	pub group: String,
	/// Rank of the environment served.
	pub rank: ServerRank,
	/// Variables belonging to the environment rather than to any one server.
	/// Every server carries these too, under its own overrides.
	pub vars: VarMap,
	/// Which of `vars` are secret.
	pub secret_vars: Vec<String>,
	/// The environment's servers, ordered by name.
	pub hosts: Vec<InventoryHost>,
}

/// Variables as a JSON object, whose values are whatever the stored tags
/// decoded to.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct VarMap(pub BTreeMap<String, Value>);

impl utoipa::PartialSchema for VarMap {
	fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
		use utoipa::openapi::schema::{AdditionalProperties, Object, SchemaType, Type};

		let mut object = Object::with_type(SchemaType::Type(Type::Object));
		object.description = Some("Variables as a JSON object.".to_string());
		object.additional_properties = Some(Box::new(AdditionalProperties::FreeForm(true)));
		utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(object))
	}
}

impl utoipa::ToSchema for VarMap {}

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

fn vars(tags: &TagMap) -> VarMap {
	VarMap(
		tags.0
			.iter()
			.filter(|(key, _)| !key.starts_with(RESERVED_TAG_PREFIX))
			.map(|(key, value)| (key.clone(), decode(value)))
			.collect(),
	)
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
/// holding several environments with no rank named, a rank with no live
/// server to configure, and a secret variable whose value cannot be read,
/// saying which it was: a refusal is a decision to respect, and a caller has to
/// be able to tell it from Canopy being unreachable.
///
/// Requires admin access, the inventory carrying the secret variables' values.
#[utoipa::path(
	post,
	path = "/for_group",
	operation_id = "inventory_for_group",
	tag = "inventory",
	security(("tailscale-admin" = [])),
	request_body = InventoryArgs,
	responses(
		(status = 200, body = InventoryView),
		(status = 400, description = "Neither or both of the group arguments", body = ProblemDetailsSchema),
		(status = 404, description = "No such server group", body = ProblemDetailsSchema),
		(status = 409, description = "Archived, empty, ambiguously named, or spanning environments", body = ProblemDetailsSchema),
		(status = 502, description = "A secret variable could not be read", body = ProblemDetailsSchema),
	),
)]
pub async fn for_group(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
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

	// A server carrying no rank is at ServerRank's own default, so every live
	// server belongs to exactly one of its group's environments.
	let ranks: BTreeSet<ServerRank> = members
		.iter()
		.map(|server| server.rank.unwrap_or_default())
		.collect();
	let rank = match args_rank {
		Some(rank) => rank,
		None if ranks.len() > 1 => {
			return Err(AppError::Conflict(format!(
				"server group {:?} holds {} environments ({}); name the rank",
				group.name,
				ranks.len(),
				ranks
					.iter()
					.map(ServerRank::to_string)
					.collect::<Vec<_>>()
					.join(", ")
			)));
		}
		None => ranks.into_iter().next().unwrap_or_default(),
	};

	let servers: Vec<Server> = members
		.into_iter()
		.filter(|server| server.rank.unwrap_or_default() == rank)
		.collect();
	if servers.is_empty() {
		return Err(AppError::Conflict(format!(
			"server group {:?} has no live server at rank {rank}",
			group.name
		)));
	}

	let device_ids: Vec<Uuid> = servers
		.iter()
		.filter_map(|server| server.device_id)
		.collect();
	let tailnet = Device::tailscale_names_by_ids(&mut conn, &device_ids).await?;

	let server_ids: Vec<Uuid> = servers.iter().map(|server| server.id).collect();
	let environment_secrets = read_secrets(
		&state,
		SecretScope::Environment {
			group_id: group.id,
			rank,
		},
		&InventorySecretVariable::list_for_environment(&mut conn, group.id, rank).await?,
	)
	.await?;
	let mut server_secrets: BTreeMap<Uuid, VarMap> = BTreeMap::new();
	for (server_id, declared) in
		by_server(InventorySecretVariable::list_for_servers(&mut conn, &server_ids).await?)
	{
		let read = read_secrets(&state, SecretScope::Server { server_id }, &declared).await?;
		server_secrets.insert(server_id, read);
	}

	tracing::info!(
		login = %admin.0.login,
		group = %group.name,
		%rank,
		secrets = environment_secrets.0.len()
			+ server_secrets.values().map(|vars| vars.0.len()).sum::<usize>(),
		"inventory served"
	);

	let mut environment_vars = vars(&group.tags);
	let environment_secret_names: Vec<String> = environment_secrets.0.keys().cloned().collect();
	environment_vars.0.extend(environment_secrets.0);

	let mut hosts: Vec<InventoryHost> = servers
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
			let own_secrets = server_secrets.remove(&server.id).unwrap_or_default();
			let mut secret_vars = environment_secret_names.clone();
			secret_vars.extend(own_secrets.0.keys().cloned());
			secret_vars.sort();
			secret_vars.dedup();

			let mut own_vars = vars(&server.tags);
			own_vars.0.extend(own_secrets.0);
			let mut effective = environment_vars.0.clone();
			effective.extend(own_vars.0.iter().map(|(k, v)| (k.clone(), v.clone())));
			InventoryHost {
				name: server
					.name
					.clone()
					.or(host)
					.unwrap_or_else(|| server.id.to_string()),
				vars: VarMap(effective),
				own_vars,
				secret_vars,
				id: server.id,
				product: server.product,
				kind: server.kind,
				address,
			}
		})
		.collect();
	hosts.sort_by(|a, b| a.name.cmp(&b.name));

	Ok(Json(InventoryView {
		group_id: group.id,
		group: group.name,
		rank,
		vars: environment_vars,
		secret_vars: environment_secret_names,
		hosts,
	}))
}

fn by_server(
	declared: Vec<InventorySecretVariable>,
) -> BTreeMap<Uuid, Vec<InventorySecretVariable>> {
	let mut out: BTreeMap<Uuid, Vec<InventorySecretVariable>> = BTreeMap::new();
	for var in declared {
		if let Some(server_id) = var.server_id {
			out.entry(server_id).or_default().push(var);
		}
	}
	out
}

/// The values behind a scope's declared names.
///
/// A name Canopy holds but cannot produce a value for refuses the whole
/// inventory: a run receiving a member that looks configured and is missing a
/// value is worse than one that does not run.
async fn read_secrets(
	state: &AppState,
	scope: SecretScope,
	declared: &[InventorySecretVariable],
) -> Result<VarMap> {
	if declared.is_empty() {
		return Ok(VarMap::default());
	}

	let kube: &BackupSecrets = state.kube.as_ref().ok_or_else(|| {
		AppError::Upstream("secret store not configured; cannot serve secret variables".into())
	})?;
	let secret_name = super::inventory_secrets::secret_name(scope);
	let held = kube.try_read_keys(&secret_name).await?.unwrap_or_default();

	let mut out = BTreeMap::new();
	for var in declared {
		let value = held.get(&var.name).ok_or_else(|| {
			AppError::Upstream(format!(
				"secret variable {:?} has no value in the secret store",
				var.name
			))
		})?;
		out.insert(var.name.clone(), decode(value));
	}
	Ok(VarMap(out))
}
