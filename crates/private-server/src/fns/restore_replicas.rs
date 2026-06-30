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
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::device::DeviceRole;
use commons_types::{
	Uuid,
	backup::{BackupType, RestoreIntent},
};
use database::diesel_async::AsyncPgConnection;
use database::pg_duration::PgDuration;
use database::{
	BackupRestoreCheck, NewRestoreReplica, RestoreConsumerCapability, RestoreReplica,
	devices::Device,
};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(for_group))
		.routes(routes!(consumers))
		.routes(routes!(checks))
		.routes(routes!(create))
		.routes(routes!(update))
		.routes(routes!(delete))
}

// ── Wire types ──────────────────────────────────────────────────────────────

/// A declared replica for the operator UI. `gap` is true when the consumer does
/// not currently advertise this declaration's intent, so Canopy is not
/// dispatching it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RestoreReplicaView {
	pub id: Uuid,
	pub consumer_device_id: Uuid,
	pub consumer_name: Option<String>,
	pub group_id: Uuid,
	pub server_id: Option<Uuid>,
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	pub name: String,
	pub freshness_seconds: Option<i64>,
	pub enabled: bool,
	pub gap: bool,
	pub created_by: Option<String>,
	#[schema(value_type = String)]
	pub created_at: Timestamp,
	#[schema(value_type = String)]
	pub updated_at: Timestamp,
}

/// A restore consumer (a `backup-restore` device) and the intents it currently
/// supports — drives the declaration form's consumer and intent pickers.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RestoreConsumerView {
	pub device_id: Uuid,
	pub name: Option<String>,
	#[schema(value_type = Vec<String>)]
	pub intents: Vec<RestoreIntent>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupArgs {
	pub server_group_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateArgs {
	pub consumer_device_id: Uuid,
	pub group_id: Uuid,
	/// `None` = all current servers in the group.
	pub server_id: Option<Uuid>,
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	pub name: String,
	/// Max snapshot age before overdue, in whole seconds; `None` = latest only.
	pub freshness_seconds: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateArgs {
	pub id: Uuid,
	pub name: String,
	pub freshness_seconds: Option<i64>,
	pub enabled: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct IdArgs {
	pub id: Uuid,
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn freshness_to_pg(seconds: Option<i64>) -> Option<PgDuration> {
	seconds.map(|s| PgDuration(SignedDuration::from_secs(s)))
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
				freshness_seconds: r.freshness.map(|f| f.0.as_secs()),
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

#[utoipa::path(
	post,
	path = "/list",
	operation_id = "restore_replicas_list",
	tag = "restore_replicas",
	security(("tailscale-user" = [])),
	responses((status = 200, body = Vec<RestoreReplicaView>)),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<RestoreReplicaView>>> {
	let mut conn = state.db.get().await?;
	let replicas = RestoreReplica::list_all(&mut conn).await?;
	Ok(Json(to_views(&mut conn, replicas).await?))
}

#[utoipa::path(
	post,
	path = "/for_group",
	operation_id = "restore_replicas_for_group",
	tag = "restore_replicas",
	security(("tailscale-user" = [])),
	request_body = GroupArgs,
	responses((status = 200, body = Vec<RestoreReplicaView>)),
)]
pub async fn for_group(
	State(state): State<AppState>,
	Json(args): Json<GroupArgs>,
) -> Result<Json<Vec<RestoreReplicaView>>> {
	let mut conn = state.db.get().await?;
	let replicas = RestoreReplica::list_for_group(&mut conn, args.server_group_id).await?;
	Ok(Json(to_views(&mut conn, replicas).await?))
}

#[utoipa::path(
	post,
	path = "/consumers",
	operation_id = "restore_replicas_consumers",
	tag = "restore_replicas",
	security(("tailscale-user" = [])),
	responses((status = 200, body = Vec<RestoreConsumerView>)),
)]
pub async fn consumers(State(state): State<AppState>) -> Result<Json<Vec<RestoreConsumerView>>> {
	let mut conn = state.db.get().await?;
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

#[utoipa::path(
	post,
	path = "/checks",
	operation_id = "restore_replicas_checks",
	tag = "restore_replicas",
	security(("tailscale-user" = [])),
	responses((status = 200, body = Vec<BackupRestoreCheck>)),
)]
pub async fn checks(State(state): State<AppState>) -> Result<Json<Vec<BackupRestoreCheck>>> {
	let mut conn = state.db.get().await?;
	let rows = BackupRestoreCheck::list_recent(&mut conn, 50).await?;
	Ok(Json(rows))
}

#[utoipa::path(
	post,
	path = "/create",
	operation_id = "restore_replicas_create",
	tag = "restore_replicas",
	security(("tailscale-admin" = [])),
	request_body = CreateArgs,
	responses(
		(status = 200, body = RestoreReplicaView),
		(status = 409, description = "A matching declaration already exists.", body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	TailscaleAdmin(admin): TailscaleAdmin,
	Json(args): Json<CreateArgs>,
) -> Result<Json<RestoreReplicaView>> {
	let mut conn = state.db.get().await?;
	let replica = RestoreReplica::create(
		&mut conn,
		NewRestoreReplica {
			consumer_device_id: args.consumer_device_id,
			group_id: args.group_id,
			server_id: args.server_id,
			r#type: args.r#type,
			intent: args.intent,
			name: args.name,
			freshness: freshness_to_pg(args.freshness_seconds),
			created_by: Some(admin.login),
		},
	)
	.await?;
	let views = to_views(&mut conn, vec![replica]).await?;
	Ok(Json(views.into_iter().next().expect("one view")))
}

#[utoipa::path(
	post,
	path = "/update",
	operation_id = "restore_replicas_update",
	tag = "restore_replicas",
	security(("tailscale-admin" = [])),
	request_body = UpdateArgs,
	responses(
		(status = 200, body = RestoreReplicaView),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<UpdateArgs>,
) -> Result<Json<RestoreReplicaView>> {
	let mut conn = state.db.get().await?;
	let replica = RestoreReplica::update(
		&mut conn,
		args.id,
		&args.name,
		freshness_to_pg(args.freshness_seconds),
		args.enabled,
	)
	.await?;
	let views = to_views(&mut conn, vec![replica]).await?;
	Ok(Json(views.into_iter().next().expect("one view")))
}

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
