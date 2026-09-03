//! Managing the secret variables an environment or an application carries.
//!
//! The name and who set it are recorded in the database; the value goes to
//! canopy's secret store and is served back only as part of an inventory (see
//! [`super::inventory`]). Nothing here returns a value.
// spec: INV#secret-variables

use axum::{Json, extract::State};
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{
	backup_secrets::BackupSecrets,
	tailscale_auth::{TailscaleAdmin, TailscaleUser},
};
use commons_types::{Uuid, server::rank::ServerRank};
use database::{
	applications::Application,
	inventory_secret_variables::{InventorySecretVariable, SecretScope},
	server_groups::ServerGroup,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(for_group))
		.routes(routes!(set))
		.routes(routes!(remove))
}

/// The Secret a scope's values live under.
pub fn secret_name(scope: SecretScope) -> String {
	match scope {
		SecretScope::Environment { group_id, rank } => {
			format!("inventory-vars-{group_id}-{rank}")
		}
		SecretScope::Application { application_id } => {
			format!("inventory-vars-application-{application_id}")
		}
	}
}

/// Which scope a request addresses: an environment, or one application.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ScopeArgs {
	/// Identifier of the server group, with `rank`, for an environment-scoped
	/// variable.
	#[serde(default)]
	pub server_group_id: Option<Uuid>,
	/// Rank of the environment, alongside `server_group_id`.
	#[serde(default)]
	pub rank: Option<ServerRank>,
	/// Identifier of the application, for an application-scoped variable.
	#[serde(default)]
	pub application_id: Option<Uuid>,
}

impl ScopeArgs {
	fn scope(&self) -> Result<SecretScope> {
		match (self.server_group_id, self.rank, self.application_id) {
			(Some(group_id), Some(rank), None) => Ok(SecretScope::Environment { group_id, rank }),
			(None, None, Some(application_id)) => Ok(SecretScope::Application { application_id }),
			(Some(_), None, None) => Err(AppError::BadRequest(
				"an environment-scoped variable needs the rank as well as the group".into(),
			)),
			_ => Err(AppError::BadRequest(
				"give either server_group_id with rank, or application_id".into(),
			)),
		}
	}
}

/// Set or replace one secret variable's value.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetArgs {
	#[serde(flatten)]
	pub scope: ScopeArgs,
	/// The variable's name.
	pub name: String,
	/// The value. Decoded on the way out the same way a tag is, so `true`,
	/// `false`, and a JSON array or object arrive as themselves.
	pub value: String,
}

/// Name one secret variable to forget.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct RemoveArgs {
	#[serde(flatten)]
	pub scope: ScopeArgs,
	/// The variable's name.
	pub name: String,
}

/// Which group's declarations to list.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SecretsForGroupArgs {
	/// Identifier of the server group.
	pub server_group_id: Uuid,
}

/// The names a group carries, at every scope under it.
///
/// Names only: a value is served solely as part of an inventory, and only to an
/// administrator. Available to any operator, so the group page can show which
/// variables are set without one.
#[utoipa::path(
	post,
	path = "/for_group",
	operation_id = "inventory_secrets_for_group",
	tag = "inventory",
	security(("tailscale-user" = [])),
	request_body = SecretsForGroupArgs,
	responses(
		(status = 200, body = Vec<InventorySecretVariable>),
		(status = 404, description = "No such server group", body = ProblemDetailsSchema),
	),
)]
pub async fn for_group(
	State(state): State<AppState>,
	_user: TailscaleUser,
	Json(args): Json<SecretsForGroupArgs>,
) -> Result<Json<Vec<InventorySecretVariable>>> {
	let mut conn = state.db.get().await?;
	ServerGroup::get_by_id(&mut conn, args.server_group_id).await?;
	Ok(Json(
		InventorySecretVariable::list_under_group(&mut conn, args.server_group_id).await?,
	))
}

/// Set or replace a secret variable.
///
/// Refuses a name already set as a tag in the same scope, and a name the secret
/// store cannot key a value under.
#[utoipa::path(
	post,
	path = "/set",
	operation_id = "inventory_secrets_set",
	tag = "inventory",
	security(("tailscale-admin" = [])),
	request_body = SetArgs,
	responses(
		(status = 200, body = InventorySecretVariable),
		(status = 400, description = "Bad scope, bad name, or a tag of that name", body = ProblemDetailsSchema),
		(status = 404, description = "No such server group or application", body = ProblemDetailsSchema),
		(status = 502, description = "The secret store is unavailable", body = ProblemDetailsSchema),
	),
)]
pub async fn set(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<SetArgs>,
) -> Result<Json<InventorySecretVariable>> {
	let scope = args.scope.scope()?;
	check_name(&args.name)?;

	let mut conn = state.db.get().await?;
	reject_tag_of_that_name(&mut conn, scope, &args.name).await?;

	let kube = secret_store(&state)?;
	let name = secret_name(scope);
	let mut keys = kube.try_read_keys(&name).await?.unwrap_or_default();
	keys.insert(args.name.clone(), args.value);
	kube.put_keys(&name, &keys).await?;

	Ok(Json(
		InventorySecretVariable::declare(&mut conn, scope, &args.name, Some(&admin.0.login))
			.await?,
	))
}

/// Forget a secret variable, value and all.
///
/// The declaration goes first, so a value the secret store will not let go of
/// leaves nothing behind that would refuse every later read of the inventory.
#[utoipa::path(
	post,
	path = "/remove",
	operation_id = "inventory_secrets_remove",
	tag = "inventory",
	security(("tailscale-admin" = [])),
	request_body = RemoveArgs,
	responses(
		(status = 200, description = "Removed", body = ()),
		(status = 400, description = "Bad scope", body = ProblemDetailsSchema),
		(status = 404, description = "No variable of that name in that scope", body = ProblemDetailsSchema),
		(status = 502, description = "The secret store is unavailable", body = ProblemDetailsSchema),
	),
)]
pub async fn remove(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<RemoveArgs>,
) -> Result<Json<()>> {
	let scope = args.scope.scope()?;
	let mut conn = state.db.get().await?;

	if !InventorySecretVariable::remove(&mut conn, scope, &args.name).await? {
		return Err(AppError::NotFound(format!(
			"no secret variable {:?} in that scope",
			args.name
		)));
	}

	let kube = secret_store(&state)?;
	let name = secret_name(scope);
	if let Some(mut keys) = kube.try_read_keys(&name).await? {
		keys.remove(&args.name);
		if keys.is_empty() {
			kube.delete_password(&name).await?;
		} else {
			kube.put_keys(&name, &keys).await?;
		}
	}
	Ok(Json(()))
}

pub(super) fn secret_store(state: &AppState) -> Result<&BackupSecrets> {
	state
		.kube
		.as_ref()
		.ok_or_else(|| AppError::Upstream("secret store not configured".into()))
}

/// The name is the key its value is stored under, so it is held to what a key
/// may be.
fn check_name(name: &str) -> Result<()> {
	if name.is_empty()
		|| !name
			.chars()
			.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
	{
		return Err(AppError::BadRequest(format!(
			"{name:?} is not a usable variable name: letters, digits, `-`, `.` and `_` only"
		)));
	}
	Ok(())
}

/// A name is a tag or a secret, never both.
async fn reject_tag_of_that_name(
	conn: &mut database::diesel_async::AsyncPgConnection,
	scope: SecretScope,
	name: &str,
) -> Result<()> {
	let carries = match scope {
		SecretScope::Environment { group_id, rank } => {
			let group = ServerGroup::get_by_id(conn, group_id).await?;
			let members = Application::list_live_in_group(conn, group_id).await?;
			group.tags.0.contains_key(name)
				|| members
					.iter()
					.filter(|application| application.rank.unwrap_or_default() == rank)
					.any(|application| application.tags.0.contains_key(name))
		}
		SecretScope::Application { application_id } => Application::get_by_id(conn, application_id)
			.await?
			.tags_merged_with_group(conn)
			.await?
			.0
			.contains_key(name),
	};

	if carries {
		return Err(AppError::BadRequest(format!(
			"{name:?} is already a tag here, so it cannot also be a secret variable"
		)));
	}
	Ok(())
}
