//! Operator-facing managed-restore endpoints (private-server, admin SPA).
//!
//! Thin wrappers over `database::restore`. Operators declare which replicas a
//! restore consumer should maintain, and see each consumer's registered
//! capabilities so the declaration UX can offer only supported intents and flag
//! declarations whose intent is currently unsupported (a *gap*).
//!
//! Reads are open to any tailnet user; mutations require admin.

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::device::DeviceRole;
use commons_types::{
	Uuid,
	backup::{BackupType, IntentDescriptor, ParamValues, RestoreIntent, validate_params},
};
use database::diesel_async::AsyncPgConnection;
use database::pg_duration::PgDuration;
use database::{
	BackupRestoreCheck, NewRestoreReplica, RestoreConsumerCapability, RestoreReplica,
	RestoreReplicaUpdate, devices::Device,
};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(for_group))
		.routes(routes!(consumers))
		.routes(routes!(checks))
		.routes(routes!(create))
		.routes(routes!(update))
		.routes(routes!(delete))
}

// ── Wire types ──────────────────────────────────────────────────────────────

/// A managed-restore declaration, as shown to operators.
///
/// A declaration instructs a restore consumer to maintain a restored replica
/// of a backup, for a given purpose (intent). It also grants the consumer
/// read access to the covered backups while it is enabled.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RestoreReplicaView {
	/// Unique identifier of the declaration.
	pub id: Uuid,
	/// Identifier of the restore consumer device the declaration is assigned to.
	pub consumer_device_id: Uuid,
	/// Display name of the consumer device, if known.
	pub consumer_name: Option<String>,
	/// Identifier of the server group whose backups the declaration covers.
	pub group_id: Uuid,
	/// Specific server within the group, or null to cover all current servers
	/// in the group.
	pub server_id: Option<Uuid>,
	/// The backup type to restore, for example `tamanu-postgres`.
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// How the replica is handled, as defined by the consumer: an arbitrary
	/// identifier from the consumer's advertised intents, e.g. `verify`.
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// Operator-chosen display name for the declaration.
	pub name: String,
	/// Overdue bound in whole seconds: how long the replica may go without a
	/// healthy restore report (or, for at-most-once intents, how long the
	/// latest snapshot may go unverified) before it is considered overdue.
	/// Null means no bound.
	pub overdue_after_seconds: Option<i64>,
	/// Operator-supplied parameter values (name → value).
	#[schema(value_type = Object)]
	pub params: serde_json::Value,
	/// Whether the declaration is active. Disabled declarations are not
	/// dispatched to the consumer and grant no backup access.
	pub enabled: bool,
	/// True when the consumer does not currently advertise this declaration's
	/// intent, so the declaration is not being dispatched.
	pub gap: bool,
	/// Login of the operator who created the declaration, if recorded.
	pub created_by: Option<String>,
	/// When the declaration was created.
	#[schema(value_type = String)]
	pub created_at: Timestamp,
	/// When the declaration was last modified.
	#[schema(value_type = String)]
	pub updated_at: Timestamp,
}

/// A restore consumer and the restore intents it currently advertises.
///
/// A restore consumer is a device with the `backup-restore` role: an agent
/// that restores backups onto standby replicas. Each advertised intent
/// carries a description, semantics flags, and a parameter schema, which
/// together determine what declarations can be created for the consumer and
/// what parameters they accept.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RestoreConsumerView {
	/// Identifier of the consumer device.
	pub device_id: Uuid,
	/// Display name of the consumer device, if known.
	pub name: Option<String>,
	/// The intents the consumer currently advertises support for.
	pub intents: Vec<IntentDescriptor>,
}

/// Scopes a request to one server group.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreReplicasGroupArgs {
	/// Identifier of the server group.
	pub server_group_id: Uuid,
}

/// Request to declare a new managed restore replica.
///
/// The consumer, group, server, backup type, and intent define the
/// declaration's scope; all of it can be changed later via `update`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreReplicasCreateArgs {
	/// Identifier of the restore consumer device to assign the declaration to.
	pub consumer_device_id: Uuid,
	/// Identifier of the server group whose backups to restore.
	pub group_id: Uuid,
	/// Specific server within the group; omit or null to cover all current
	/// servers in the group.
	pub server_id: Option<Uuid>,
	/// The backup type to restore, for example `tamanu-postgres`.
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// How the replica is handled, as defined by the consumer: an arbitrary
	/// identifier from the consumer's advertised intents, e.g. `verify`.
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// Display name for the declaration.
	pub name: String,
	/// Overdue bound in whole seconds; omit or null for no bound.
	pub overdue_after_seconds: Option<i64>,
	/// Parameter values for the intent (name → value), validated against the
	/// consumer's advertised parameter schema. Defaults to empty.
	#[serde(default)]
	#[schema(value_type = Object)]
	pub params: ParamValues,
}

/// Request to update an existing declaration.
///
/// Replaces every field, including scope: the consumer, group, server,
/// backup type, and intent can all be changed in the same call as the name,
/// overdue bound, parameter values, and enabled flag. A scope that collides
/// with another declaration's `(consumer, group, type, intent, server)` maps
/// to `409`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreReplicasUpdateArgs {
	/// Identifier of the declaration to update.
	pub id: Uuid,
	/// Identifier of the restore consumer device to assign the declaration to.
	pub consumer_device_id: Uuid,
	/// Identifier of the server group whose backups to restore.
	pub group_id: Uuid,
	/// Specific server within the group; omit or null to cover all current
	/// servers in the group.
	pub server_id: Option<Uuid>,
	/// The backup type to restore, for example `tamanu-postgres`.
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// How the replica is handled, as defined by the consumer: an arbitrary
	/// identifier from the consumer's advertised intents, e.g. `verify`.
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// New display name for the declaration.
	pub name: String,
	/// New overdue bound in whole seconds; null removes the bound.
	pub overdue_after_seconds: Option<i64>,
	/// New parameter values (name → value), validated against the intent's
	/// advertised parameter schema. Defaults to empty.
	#[serde(default)]
	#[schema(value_type = Object)]
	pub params: ParamValues,
	/// Whether the declaration should be active.
	pub enabled: bool,
}

/// Identifies the declaration to operate on.
#[derive(Debug, Deserialize, ToSchema)]
pub struct IdArgs {
	/// Identifier of the declaration.
	pub id: Uuid,
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn overdue_after_to_pg(seconds: Option<i64>) -> Option<PgDuration> {
	seconds.map(|s| PgDuration(SignedDuration::from_secs(s)))
}

/// Validate operator-supplied parameter values against the consumer's advertised
/// schema for `intent`. If the intent is not advertised and `require_advertised`
/// is `false`, there is no schema to check against, so the values are accepted
/// as-is (used by `create`, which allows declaring ahead of the consumer
/// registering support, surfaced as a gap). If `require_advertised` is `true`,
/// an unadvertised intent is rejected outright (used by `update`, where the
/// operator is explicitly retargeting a live declaration onto a consumer or
/// intent that cannot currently serve it).
async fn validate_params_for_intent(
	conn: &mut AsyncPgConnection,
	consumer_device_id: Uuid,
	intent: &RestoreIntent,
	params: &ParamValues,
	require_advertised: bool,
) -> Result<()> {
	let descriptors =
		RestoreConsumerCapability::list_for_consumer(conn, consumer_device_id).await?;
	match descriptors.iter().find(|d| &d.intent == intent) {
		Some(desc) => {
			validate_params(&desc.params, params)
				.map_err(|e| AppError::BadRequest(e.to_string()))?;
		}
		None if require_advertised => {
			return Err(AppError::BadRequest(format!(
				"consumer does not currently advertise intent {intent:?}"
			)));
		}
		None => {}
	}
	Ok(())
}

/// Build views from declarations, resolving consumer display names and the
/// per-consumer capability set so `gap` can be computed.
async fn to_views(
	conn: &mut AsyncPgConnection,
	replicas: Vec<RestoreReplica>,
) -> Result<Vec<RestoreReplicaView>> {
	let consumer_ids: HashSet<Uuid> = replicas.iter().map(|r| r.consumer_device_id).collect();

	// Consumer display names come from the set of restore-consumer devices.
	let names: HashMap<Uuid, Option<String>> =
		Device::list_by_role(conn, DeviceRole::BackupRestore)
			.await?
			.into_iter()
			.map(|d| (d.id, d.tailscale_node_name))
			.collect();

	let mut caps: HashMap<Uuid, HashSet<RestoreIntent>> = HashMap::new();
	for id in consumer_ids {
		let set: HashSet<RestoreIntent> = RestoreConsumerCapability::list_for_consumer(conn, id)
			.await?
			.into_iter()
			.map(|d| d.intent)
			.collect();
		caps.insert(id, set);
	}

	Ok(replicas
		.into_iter()
		.map(|r| {
			let gap = !caps
				.get(&r.consumer_device_id)
				.map(|s| s.contains(&r.intent))
				.unwrap_or(false);
			RestoreReplicaView {
				consumer_name: names.get(&r.consumer_device_id).cloned().flatten(),
				overdue_after_seconds: r.overdue_after.map(|f| f.0.as_secs()),
				params: r.params,
				gap,
				id: r.id,
				consumer_device_id: r.consumer_device_id,
				group_id: r.group_id,
				server_id: r.server_id,
				r#type: r.r#type,
				intent: r.intent,
				name: r.name,
				enabled: r.enabled,
				created_by: r.created_by,
				created_at: r.created_at,
				updated_at: r.updated_at,
			}
		})
		.collect())
}

// ── Handlers ──────────────────────────────────────────────────────────────

/// List restore replica declarations for a group.
///
/// Returns every declaration scoped to the given server group, with each
/// consumer's display name resolved and the `gap` flag computed against the
/// intents the consumer currently advertises.
#[utoipa::path(
	post,
	path = "/for_group",
	operation_id = "restore_replicas_for_group",
	tag = "restore_replicas",
	security(("tailscale-user" = [])),
	request_body = RestoreReplicasGroupArgs,
	responses((status = 200, body = Vec<RestoreReplicaView>)),
)]
pub async fn for_group(
	State(state): State<AppState>,
	Json(args): Json<RestoreReplicasGroupArgs>,
) -> Result<Json<Vec<RestoreReplicaView>>> {
	let mut conn = state.db_read.get().await?;
	let replicas = RestoreReplica::list_for_group(&mut conn, args.server_group_id).await?;
	Ok(Json(to_views(&mut conn, replicas).await?))
}

/// List restore consumers and their advertised intents.
///
/// Returns every device with the backup-restore role, together with the
/// restore intents it currently advertises (each with its description,
/// semantics flags, and parameter schema). Use this to discover which
/// consumers and intents a declaration can target and which parameters each
/// intent accepts.
#[utoipa::path(
	post,
	path = "/consumers",
	operation_id = "restore_replicas_consumers",
	tag = "restore_replicas",
	security(("tailscale-user" = [])),
	responses((status = 200, body = Vec<RestoreConsumerView>)),
)]
pub async fn consumers(State(state): State<AppState>) -> Result<Json<Vec<RestoreConsumerView>>> {
	let mut conn = state.db_read.get().await?;
	let devices = Device::list_by_role(&mut conn, DeviceRole::BackupRestore).await?;
	let mut out = Vec::with_capacity(devices.len());
	for d in devices {
		let intents = RestoreConsumerCapability::list_for_consumer(&mut conn, d.id).await?;
		out.push(RestoreConsumerView {
			device_id: d.id,
			name: d.tailscale_node_name,
			intents,
		});
	}
	Ok(Json(out))
}

/// List recent restore-health reports for a group.
///
/// Returns up to the 50 most recent restore-health reports submitted by
/// consumers for the given server group. Each report records whether a
/// backup snapshot restored successfully and whether the resulting replica
/// was healthy — the strongest available signal that the group's backups are
/// actually restorable.
#[utoipa::path(
	post,
	path = "/checks",
	operation_id = "restore_replicas_checks",
	tag = "restore_replicas",
	security(("tailscale-user" = [])),
	request_body = RestoreReplicasGroupArgs,
	responses((status = 200, body = Vec<BackupRestoreCheck>)),
)]
pub async fn checks(
	State(state): State<AppState>,
	Json(args): Json<RestoreReplicasGroupArgs>,
) -> Result<Json<Vec<BackupRestoreCheck>>> {
	let mut conn = state.db_read.get().await?;
	let rows =
		BackupRestoreCheck::list_recent_for_group(&mut conn, args.server_group_id, 50).await?;
	Ok(Json(rows))
}

/// Declare a managed restore replica.
///
/// Creates a declaration instructing the chosen consumer to maintain a
/// restored replica of the given backup type for the given intent, and
/// records the calling operator as its creator. Parameter values are
/// validated against the consumer's advertised schema for the intent; if the
/// intent is not currently advertised, the values are accepted as-is and the
/// declaration is created with a gap. Requires the caller to be on the admin
/// allow-list. Responds 400 if a parameter value fails validation and 409 if
/// a matching declaration already exists.
#[utoipa::path(
	post,
	path = "/create",
	operation_id = "restore_replicas_create",
	tag = "restore_replicas",
	security(("tailscale-admin" = [])),
	request_body = RestoreReplicasCreateArgs,
	responses(
		(status = 200, body = RestoreReplicaView),
		(status = 409, description = "A matching declaration already exists.", body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<RestoreReplicasCreateArgs>,
) -> Result<Json<RestoreReplicaView>> {
	let mut conn = state.db.get().await?;
	validate_params_for_intent(
		&mut conn,
		args.consumer_device_id,
		&args.intent,
		&args.params,
		false,
	)
	.await?;
	let replica = RestoreReplica::create(
		&mut conn,
		NewRestoreReplica {
			consumer_device_id: args.consumer_device_id,
			group_id: args.group_id,
			server_id: args.server_id,
			r#type: args.r#type,
			intent: args.intent,
			name: args.name,
			overdue_after: overdue_after_to_pg(args.overdue_after_seconds),
			params: serde_json::to_value(&args.params).expect("params serialize"),
			created_by: Some(admin.login),
		},
	)
	.await?;
	let views = to_views(&mut conn, vec![replica]).await?;
	Ok(Json(views.into_iter().next().expect("one view")))
}

/// Update a restore replica declaration.
///
/// Replaces every field, including scope: the consumer, group, server,
/// backup type, and intent can be retargeted in the same call as the name,
/// overdue bound, parameter values, and enabled flag. Parameter values are
/// validated against the *new* consumer+intent's advertised parameter schema;
/// unlike `create`, an intent the new consumer doesn't currently advertise is
/// rejected rather than accepted as a gap, since this is an explicit
/// retargeting of a live declaration rather than an initial declaration made
/// ahead of the consumer registering support. If the scope changes, any
/// active restore-verification alert for the declaration's old scope is
/// recovered. Requires the caller to be on the admin allow-list. Responds 400
/// if a parameter value fails validation or the new intent isn't advertised,
/// 404 if the declaration does not exist, and 409 if the new scope collides
/// with another declaration.
#[utoipa::path(
	post,
	path = "/update",
	operation_id = "restore_replicas_update",
	tag = "restore_replicas",
	security(("tailscale-admin" = [])),
	request_body = RestoreReplicasUpdateArgs,
	responses(
		(status = 200, body = RestoreReplicaView),
		(status = 404, body = ProblemDetailsSchema),
		(status = 409, description = "The new scope collides with another declaration.", body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<RestoreReplicasUpdateArgs>,
) -> Result<Json<RestoreReplicaView>> {
	let mut conn = state.db.get().await?;
	validate_params_for_intent(
		&mut conn,
		args.consumer_device_id,
		&args.intent,
		&args.params,
		true,
	)
	.await?;
	let replica = RestoreReplica::update(
		&mut conn,
		args.id,
		RestoreReplicaUpdate {
			consumer_device_id: args.consumer_device_id,
			group_id: args.group_id,
			server_id: args.server_id,
			r#type: args.r#type,
			intent: args.intent,
			name: args.name,
			overdue_after: overdue_after_to_pg(args.overdue_after_seconds),
			params: serde_json::to_value(&args.params).expect("params serialize"),
			enabled: args.enabled,
		},
	)
	.await?;
	let views = to_views(&mut conn, vec![replica]).await?;
	Ok(Json(views.into_iter().next().expect("one view")))
}

/// Delete a restore replica declaration.
///
/// Removes the declaration: the consumer stops being asked to maintain the
/// replica and loses the backup access the declaration granted. Requires the
/// caller to be on the admin allow-list. Responds 404 if the declaration
/// does not exist.
#[utoipa::path(
	post,
	path = "/delete",
	operation_id = "restore_replicas_delete",
	tag = "restore_replicas",
	security(("tailscale-admin" = [])),
	request_body = IdArgs,
	responses((status = 200), (status = 404, body = ProblemDetailsSchema)),
)]
pub async fn delete(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<IdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	RestoreReplica::delete(&mut conn, args.id).await?;
	Ok(Json(()))
}
