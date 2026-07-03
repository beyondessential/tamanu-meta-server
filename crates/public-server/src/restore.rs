//! Managed-restore endpoints (RST) — the consumer-facing side of the restore
//! control plane. All `BackupRestoreDevice`-authenticated, mounted at the root:
//!
//! - `POST /restore-capabilities` — the consumer registers the intents it can
//!   satisfy. Canopy dispatches only matching worklist entries.
//! - `GET  /restore-worklist` — the consumer's complete desired state: its
//!   enabled declarations expanded per current server, each carrying the
//!   snapshot Canopy wants restored and the repo coordinates to find it.
//! - `POST /restore-credentials` — short-lived **read-only** S3 creds plus the
//!   repo password for one `(group, type)` the consumer is authorized for.
//!
//! The `backup-restore` role is read-only by construction: it cannot reach the
//! `ServerDevice`-gated `/backup-credentials`, and `/restore-credentials` only
//! ever issues the read-only [`restore_session_policy`].

use aws_sdk_sts::operation::RequestId as _;
use axum::{Json, extract::State, http::StatusCode};
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::BackupRestoreDevice;
use commons_types::backup::{
	BackupPurpose, BackupType, IntentDescriptor, ParamValues, RestoreIntent, RunOutcome,
	resolve_params, semantics,
};
use database::{
	Db,
	backups::{BackupRun, NewBackupCredentialIssuance, ServerGroupBackupConfig},
	restore::{
		BackupRestoreCheck, NewBackupRestoreCheck, RestoreConsumerCapability, RestoreReplica,
	},
	servers::Server,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
	backup::{
		CredentialProcessOutput, REPO_PASSWORD_SECRET_KEY, deployment_default_region,
		restore_session_policy,
	},
	state::{AppState, BackupSecrets},
};

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(capabilities))
		.routes(routes!(worklist))
		.routes(routes!(credentials))
		.routes(routes!(verification))
}

// ---------------------------------------------------------------------------
// POST /restore-capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreCapabilitiesArgs {
	/// The intents this consumer can satisfy, each with its description, the
	/// Canopy semantics it opts into, and its parameter schema. Replaces the
	/// consumer's advertised set wholesale.
	pub intents: Vec<IntentDescriptor>,
}

#[utoipa::path(
	post,
	path = "/restore-capabilities",
	operation_id = "register_restore_capabilities",
	tag = "restore",
	security(("backup-restore-device" = [])),
	request_body = RestoreCapabilitiesArgs,
	responses((status = 204, description = "Capability set registered.")),
)]
async fn capabilities(
	State(db): State<Db>,
	device: BackupRestoreDevice,
	Json(args): Json<RestoreCapabilitiesArgs>,
) -> Result<StatusCode> {
	let mut conn = db.get().await?;
	let consumer_device_id = device.0.0.id;
	RestoreConsumerCapability::register(&mut conn, consumer_device_id, &args.intents).await?;
	Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /restore-worklist
// ---------------------------------------------------------------------------

/// One concrete replica the consumer should maintain: a declaration expanded
/// against a single server, carrying the snapshot to restore and the repo
/// coordinates to find it. Credentials and the repo password are obtained
/// separately via `/restore-credentials`.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorklistEntry {
	/// The declaration this entry came from.
	pub replica_id: Uuid,
	pub group_id: Uuid,
	pub server_id: Uuid,
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	pub name: String,
	/// The overdue bound in whole seconds; `None` = no bound. Interpreted per the
	/// intent's semantics (a `once` snapshot bound, or a standing staleness
	/// bound).
	pub overdue_after_seconds: Option<i64>,
	/// The resolved parameter values for this replica: one per parameter the
	/// intent advertises, unset ones filled with their default or JSON `null`.
	#[schema(value_type = Object)]
	pub params: ParamValues,
	/// The snapshot Canopy wants restored — the latest successful backup for
	/// this `(server, type)`. `None` when no successful backup is yet known.
	pub snapshot_id: Option<String>,
	/// RFC3339 timestamp of that snapshot, if known.
	pub snapshot_at: Option<String>,
	/// Always `"s3"`.
	pub storage: String,
	pub bucket: String,
	pub prefix: String,
	pub region: String,
}

#[utoipa::path(
	get,
	path = "/restore-worklist",
	tag = "restore",
	security(("backup-restore-device" = [])),
	responses((status = 200, body = Vec<WorklistEntry>)),
)]
async fn worklist(
	State(db): State<Db>,
	device: BackupRestoreDevice,
) -> Result<Json<Vec<WorklistEntry>>> {
	let mut conn = db.get().await?;
	let consumer_device_id = device.0.0.id;

	// Only intents the consumer currently advertises are dispatched; a
	// declaration on an unadvertised intent is a gap, surfaced to operators,
	// never sent here. Keep the descriptors to read semantics and parameter
	// schemas per intent.
	let descriptors: HashMap<RestoreIntent, IntentDescriptor> =
		RestoreConsumerCapability::list_for_consumer(&mut conn, consumer_device_id)
			.await?
			.into_iter()
			.map(|d| (d.intent.clone(), d))
			.collect();

	let mut declarations = RestoreReplica::list_enabled_for_consumer(&mut conn, consumer_device_id)
		.await?
		.into_iter()
		.filter(|d| descriptors.contains_key(&d.intent))
		.collect::<Vec<_>>();
	// Process server-specific declarations before group-wide ones so a
	// server-scoped declaration wins the dedup over a group-wide one covering
	// the same (server, type, intent).
	declarations.sort_by_key(|d| d.server_id.is_none());

	let mut out: Vec<WorklistEntry> = Vec::new();
	let mut seen: HashSet<(Uuid, String, String)> = HashSet::new();
	// Per-group caches so a group referenced by several declarations is resolved
	// once: the latest produced snapshot per (server, type), and the latest
	// healthy-verified snapshot per (server, type, intent) for `once` suppression.
	let mut snapshot_cache: HashMap<Uuid, HashMap<(Uuid, BackupType), BackupRun>> = HashMap::new();
	let mut verified_cache: HashMap<Uuid, HashMap<(Uuid, BackupType, RestoreIntent), String>> =
		HashMap::new();

	for d in declarations {
		// A worklist entry needs somewhere to restore from: skip groups without
		// a ready config (they surface elsewhere as not-yet-restorable).
		let Some(cfg) = ServerGroupBackupConfig::get(&mut conn, d.group_id).await? else {
			continue;
		};
		if cfg.status != commons_types::backup::BackupConfigStatus::Ready {
			continue;
		}

		let servers = match d.server_id {
			Some(sid) => {
				let s = Server::get_by_id(&mut conn, sid).await?;
				// Skip a declaration whose server has left the group or been
				// archived; it lingers as a no-op until the operator retires it.
				if s.group_id == Some(d.group_id) && s.deleted_at.is_none() {
					vec![s]
				} else {
					vec![]
				}
			}
			None => Server::list_live_in_group(&mut conn, d.group_id).await?,
		};

		if !snapshot_cache.contains_key(&d.group_id) {
			snapshot_cache.insert(
				d.group_id,
				BackupRun::latest_success_by_server_type_for_group(&mut conn, d.group_id).await?,
			);
			verified_cache.insert(
				d.group_id,
				BackupRestoreCheck::latest_healthy_snapshot_by_key_for_group(&mut conn, d.group_id)
					.await?,
			);
		}
		let snapshots = &snapshot_cache[&d.group_id];
		let verified = &verified_cache[&d.group_id];

		// The descriptor governs the intent's semantics and parameter resolution.
		let descriptor = &descriptors[&d.intent];
		let once = descriptor.has_semantic(semantics::ONCE);
		let replica_values: ParamValues =
			serde_json::from_value(d.params.clone()).unwrap_or_default();
		let params = resolve_params(&descriptor.params, &replica_values);

		let region = cfg.region.clone().unwrap_or_else(deployment_default_region);
		for server in servers {
			let key = (server.id, d.r#type.to_string(), d.intent.to_string());
			if !seen.insert(key) {
				continue;
			}
			let latest = snapshots.get(&(server.id, d.r#type.clone()));
			// A `once` intent drops off the worklist once the latest snapshot has
			// a healthy report; it reappears only when a newer snapshot exists.
			if once {
				let key = (server.id, d.r#type.clone(), d.intent.clone());
				let already = matches!(
					(verified.get(&key), latest.and_then(|r| r.snapshot_id.as_ref())),
					(Some(v), Some(s)) if v == s
				);
				if already {
					continue;
				}
			}
			out.push(WorklistEntry {
				replica_id: d.id,
				group_id: d.group_id,
				server_id: server.id,
				r#type: d.r#type.clone(),
				intent: d.intent.clone(),
				name: d.name.clone(),
				overdue_after_seconds: d.overdue_after.map(|f| f.0.as_secs()),
				params: params.clone(),
				snapshot_id: latest.and_then(|r| r.snapshot_id.clone()),
				snapshot_at: latest.map(|r| r.reported_at.to_string()),
				storage: "s3".into(),
				bucket: cfg.bucket.clone(),
				prefix: cfg.prefix.clone(),
				region: region.clone(),
			});
		}
	}

	Ok(Json(out))
}

// ---------------------------------------------------------------------------
// POST /restore-credentials
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreCredentialsArgs {
	/// The group whose repo to read.
	pub group: Uuid,
	/// The backup type to restore.
	#[schema(value_type = String)]
	pub r#type: BackupType,
}

/// Read-only credentials plus the repo password for one `(group, type)`. The
/// AWS creds are the `credential_process` shape the consumer's proxy refreshes;
/// the password opens the kopia repo.
#[derive(Debug, Serialize, ToSchema)]
pub struct RestoreCredentials {
	pub credentials: CredentialProcessOutput,
	/// The kopia repo passphrase, read from the group's k8s Secret.
	pub repo_password: String,
}

#[utoipa::path(
	post,
	path = "/restore-credentials",
	operation_id = "mint_restore_credentials",
	tag = "restore",
	security(("backup-restore-device" = [])),
	request_body = RestoreCredentialsArgs,
	responses(
		(status = 200, body = RestoreCredentials),
		(status = 403, description = "No enabled declaration authorizes this (group, type).", body = ProblemDetailsSchema),
		(status = 409, description = "Group has no ready backup config.", body = ProblemDetailsSchema),
		(status = 502, description = "STS issuance or repo-password read failed or is not configured.", body = ProblemDetailsSchema),
	),
)]
async fn credentials(
	State(db): State<Db>,
	State(sts): State<Option<aws_sdk_sts::Client>>,
	State(kube): State<Option<BackupSecrets>>,
	device: BackupRestoreDevice,
	Json(args): Json<RestoreCredentialsArgs>,
) -> Result<Json<RestoreCredentials>> {
	let mut conn = db.get().await?;
	let consumer_device_id = device.0.0.id;

	// Authorization is the declared replica: a consumer may read exactly the
	// (group, type) pairs its enabled declarations cover.
	if !RestoreReplica::authorizes(&mut conn, consumer_device_id, args.group, &args.r#type).await? {
		return Err(AppError::AuthInsufficientPermissions {
			required: "an enabled restore-replica declaration for this group and type".into(),
		});
	}

	let cfg = ServerGroupBackupConfig::get(&mut conn, args.group)
		.await?
		.ok_or_else(|| AppError::Conflict("group has no backup config".into()))?;
	if cfg.status != commons_types::backup::BackupConfigStatus::Ready {
		return Err(AppError::Conflict(
			"group backup config is not ready".into(),
		));
	}

	// Always read-only — this role cannot mint write creds.
	let session_policy = restore_session_policy(&cfg.bucket, &cfg.prefix);

	let Some(sts) = sts else {
		tracing::error!(group = %args.group, "restore-credentials: STS client not configured");
		return Err(AppError::Upstream(
			"credential issuer not configured".into(),
		));
	};

	let session_name = format!("canopy-restore-{consumer_device_id}");
	let resp = sts
		.assume_role()
		.role_arn(&cfg.target_role_arn)
		.role_session_name(session_name)
		.policy(session_policy)
		.duration_seconds(3600)
		.send()
		.await
		.map_err(|err| {
			let request_id = err.request_id().unwrap_or("<none>");
			tracing::error!(
				group = %args.group,
				role = %cfg.target_role_arn,
				request_id,
				error = ?err,
				"restore-credentials: AssumeRole failed",
			);
			AppError::Upstream("credential issuance failed".into())
		})?;

	let sts_request_id = resp.request_id().map(str::to_owned);
	let creds = resp.credentials().ok_or_else(|| {
		tracing::error!(group = %args.group, "restore-credentials: AssumeRole returned no credentials");
		AppError::Upstream("credential issuance returned no credentials".into())
	})?;

	let expiry_secs = creds.expiration().secs();
	let expires_at = Timestamp::from_second(expiry_secs).map_err(|err| {
		tracing::error!(group = %args.group, error = ?err, "restore-credentials: bad expiration");
		AppError::Upstream("credential issuance returned an invalid expiration".into())
	})?;
	let access_key_id = creds.access_key_id().to_owned();

	let Some(kube) = kube else {
		tracing::error!(group = %args.group, "restore-credentials: kube client not configured");
		return Err(AppError::Upstream("secret store not configured".into()));
	};
	let repo_password = kube
		.read_password(&cfg.repo_password_ref, REPO_PASSWORD_SECRET_KEY)
		.await
		.map_err(|err| {
			tracing::error!(
				group = %args.group,
				secret = %cfg.repo_password_ref,
				error = ?err,
				"restore-credentials: reading repo-password Secret failed",
			);
			AppError::Upstream("repo password unavailable".into())
		})?;

	// Audit BEFORE returning — never hand out creds we didn't record.
	database::backups::BackupCredentialIssuance::record(
		&mut conn,
		NewBackupCredentialIssuance {
			device_id: consumer_device_id,
			group_id: args.group,
			r#type: args.r#type.clone(),
			expires_at,
			purpose: BackupPurpose::Restore,
			sts_assumed_role: cfg.target_role_arn.clone(),
			sts_request_id,
			access_key_id: Some(access_key_id.clone()),
			bucket: cfg.bucket.clone(),
			prefix: cfg.prefix.clone(),
		},
	)
	.await?;

	Ok(Json(RestoreCredentials {
		credentials: CredentialProcessOutput {
			version: 1,
			access_key_id,
			secret_access_key: creds.secret_access_key().to_owned(),
			session_token: creds.session_token().to_owned(),
			expiration: expires_at.to_string(),
		},
		repo_password,
	}))
}

// ---------------------------------------------------------------------------
// POST /restore-verification
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct VerificationArgs {
	/// The declaration this report concerns (from the worklist entry); optional
	/// so a report survives the declaration being retired mid-flight.
	pub replica_id: Option<Uuid>,
	pub group: Uuid,
	pub server_id: Uuid,
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// The snapshot that was restored; omit on a failure that never got there.
	pub snapshot_id: Option<String>,
	#[schema(value_type = String)]
	pub outcome: RunOutcome,
	pub error: Option<String>,
	/// Whether the restored database came up healthy and passed readiness.
	pub replica_healthy: bool,
	pub postgres_version: Option<String>,
	/// When the restore was observed (RFC3339).
	#[schema(value_type = String)]
	pub observed_at: Timestamp,
	pub s3_sent_raw_bytes: Option<i64>,
	pub s3_sent_payload_bytes: Option<i64>,
	pub s3_received_raw_bytes: Option<i64>,
	pub s3_received_payload_bytes: Option<i64>,
	/// Arbitrary health data to record alongside the check (postgres cluster
	/// stats, whether indexes needed fixing, …). Stored and displayed as-is.
	pub health_details: Option<serde_json::Value>,
}

#[utoipa::path(
	post,
	path = "/restore-verification",
	tag = "restore",
	security(("backup-restore-device" = [])),
	request_body = VerificationArgs,
	responses(
		(status = 204, description = "Report recorded."),
		(status = 403, description = "No enabled declaration authorizes this (group, type).", body = ProblemDetailsSchema),
	),
)]
async fn verification(
	State(db): State<Db>,
	device: BackupRestoreDevice,
	Json(args): Json<VerificationArgs>,
) -> Result<StatusCode> {
	let mut conn = db.get().await?;
	let consumer_device_id = device.0.0.id;

	// Same authorization as credentials: a consumer may report only on the
	// (group, type) pairs its enabled declarations cover.
	if !RestoreReplica::authorizes(&mut conn, consumer_device_id, args.group, &args.r#type).await? {
		return Err(AppError::AuthInsufficientPermissions {
			required: "an enabled restore-replica declaration for this group and type".into(),
		});
	}

	BackupRestoreCheck::record_report(
		&mut conn,
		NewBackupRestoreCheck {
			replica_id: args.replica_id,
			consumer_device_id,
			group_id: args.group,
			server_id: Some(args.server_id),
			r#type: args.r#type,
			intent: args.intent,
			snapshot_id: args.snapshot_id,
			outcome: args.outcome,
			error: args.error,
			replica_healthy: args.replica_healthy,
			postgres_version: args.postgres_version,
			observed_at: args.observed_at,
			s3_sent_raw_bytes: args.s3_sent_raw_bytes,
			s3_sent_payload_bytes: args.s3_sent_payload_bytes,
			s3_received_raw_bytes: args.s3_received_raw_bytes,
			s3_received_payload_bytes: args.s3_received_payload_bytes,
			health_details: args.health_details,
		},
	)
	.await?;

	Ok(StatusCode::NO_CONTENT)
}
