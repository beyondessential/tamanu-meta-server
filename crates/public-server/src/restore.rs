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

/// Request body for registering the restore intents a consumer device can
/// satisfy.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreCapabilitiesArgs {
	/// The intents this device can satisfy — arbitrary consumer-chosen
	/// identifiers (e.g. `verify`) — each with its description, the semantics
	/// it opts into, and the schema of the parameters it accepts. Replaces the
	/// device's previously advertised set wholesale.
	pub intents: Vec<IntentDescriptor>,
}

/// Register the restore intents this device can satisfy.
///
/// Declares the restore intents the calling device supports, replacing any
/// previously advertised set. Only worklist entries whose intent is currently
/// advertised are dispatched to this device via `GET /restore-worklist`, so
/// register on startup and whenever the supported set changes.
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

/// One replica the consumer device should currently maintain: an operator
/// declaration expanded against a single server, carrying the snapshot to
/// restore and the repository coordinates to find it. S3 credentials and the
/// repository passphrase are obtained separately via
/// `POST /restore-credentials`.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorklistEntry {
	/// Identifier of the declaration this entry was expanded from. Echo it
	/// back in `POST /restore-verification` reports.
	pub replica_id: Uuid,
	/// The server group whose backup repository holds the snapshot.
	pub group_id: Uuid,
	/// The server whose backup should be restored.
	pub server_id: Uuid,
	/// The backup type to restore (e.g. `tamanu-postgres`).
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// The restore intent this entry is for; one of the intents this device
	/// advertised via `POST /restore-capabilities`.
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// Operator-assigned label for the declaration.
	pub name: String,
	/// Bound, in whole seconds, after which the replica counts as overdue;
	/// `null` means no bound. Interpreted per the intent's semantics: for a
	/// run-once (`once`) intent, how long the latest snapshot may go without a
	/// healthy verification report; for a standing replica, how stale its last
	/// healthy report may be.
	pub overdue_after_seconds: Option<i64>,
	/// Resolved parameter values for this replica: one key per parameter the
	/// intent advertises. Parameters the operator left unset carry the
	/// intent's declared default, or JSON `null` when there is none.
	#[schema(value_type = Object)]
	pub params: ParamValues,
	/// Identifier of the snapshot to restore — the latest successful backup
	/// for this server and type. `null` when no successful backup is known
	/// yet.
	pub snapshot_id: Option<String>,
	/// When that snapshot was reported, as an RFC 3339 timestamp; `null` if
	/// unknown.
	pub snapshot_at: Option<String>,
	/// Kind of storage backend. Always `"s3"`.
	pub storage: String,
	/// Name of the S3 bucket holding the group's backup repository.
	pub bucket: String,
	/// Key prefix within the bucket under which the repository lives. Normally
	/// empty (the repository is at the bucket root).
	pub prefix: String,
	/// AWS region of the bucket.
	pub region: String,
}

/// Fetch the full set of replicas this device should maintain.
///
/// Returns the device's complete desired state, computed fresh on every call:
/// each enabled restore declaration whose intent this device currently
/// advertises, expanded into one entry per server it covers. A group-wide
/// declaration expands to every live server in its group; a server-scoped
/// declaration yields a single entry and takes precedence over a group-wide
/// one covering the same server, type, and intent. Entries for groups whose
/// backup configuration is not ready are omitted, and entries for run-once
/// intents disappear once the latest snapshot has a healthy verification
/// report, reappearing when a newer snapshot exists.
///
/// An empty array means there is nothing to do. Poll this endpoint and
/// reconcile: create or refresh the replicas listed, and tear down any the
/// device is maintaining that no longer appear.
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

/// Request body for minting read-only restore credentials.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreCredentialsArgs {
	/// The server group whose backup repository to read.
	pub group: Uuid,
	/// The backup type to restore (e.g. `tamanu-postgres`).
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// This must be the run-uuid the client minted for this run.
	/// The field is optional only so older clients don't break; it WILL be made
	/// mandatory in future.
	pub run_id: Option<Uuid>,
}

/// Read-only S3 credentials plus the repository passphrase for one group and
/// backup type: everything needed to open the group's backup repository and
/// read a snapshot out of it.
#[derive(Debug, Serialize, ToSchema)]
pub struct RestoreCredentials {
	/// Temporary read-only AWS credentials in the `credential_process` output
	/// format, valid for at most one hour.
	pub credentials: CredentialProcessOutput,
	/// Passphrase for the group's backup repository (a Kopia repository).
	pub repo_password: String,
}

/// Mint read-only credentials for a group's backup repository.
///
/// Issues temporary AWS credentials — always strictly read-only, scoped to the
/// group's backup storage — together with the repository passphrase, so the
/// device can read the snapshot named in a worklist entry. Credentials expire
/// after at most one hour; request a fresh set per restore rather than caching
/// them. Every issuance is recorded for audit.
///
/// The device must hold an enabled restore declaration covering the requested
/// group and type (i.e. the pair must appear in its worklist configuration);
/// otherwise the request is rejected with 403.
///
/// Errors: 403 when no enabled declaration authorizes this group and type;
/// 409 when the group has no ready backup configuration; 502 when the
/// credential issuer or the passphrase store is unavailable or not configured.
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
			run_id: args.run_id,
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

/// Report of a restore attempt and the health of the resulting replica.
#[derive(Debug, Deserialize, ToSchema)]
pub struct VerificationArgs {
	/// The declaration this report concerns, taken from the worklist entry's
	/// `replica_id`. Optional so a report is still accepted when the
	/// declaration was retired while the restore was in flight.
	pub replica_id: Option<Uuid>,
	/// The server group whose backup was restored.
	pub group: Uuid,
	/// The server whose backup was restored.
	pub server_id: Uuid,
	/// The backup type that was restored (e.g. `tamanu-postgres`).
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// The restore intent this attempt was performed under.
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// Identifier of the snapshot that was restored. Omit on a failure that
	/// never got as far as selecting a snapshot.
	pub snapshot_id: Option<String>,
	/// Whether the restore succeeded (`success`) or failed (`failure`).
	#[schema(value_type = String)]
	pub outcome: RunOutcome,
	/// Human-readable error detail, when the restore failed.
	pub error: Option<String>,
	/// Whether the restored database came up healthy and passed readiness
	/// checks. A replica only counts as verified when the outcome is
	/// `success` and this is `true`.
	pub replica_healthy: bool,
	/// Version of the PostgreSQL server the data was restored into, if
	/// applicable.
	pub postgres_version: Option<String>,
	/// When the restore result was observed, as an RFC 3339 timestamp.
	#[schema(value_type = String)]
	pub observed_at: Timestamp,
	/// Bytes of raw HTTP traffic sent to S3 during the restore, including
	/// protocol and signing overhead. Omit when traffic was not measured.
	pub s3_sent_raw_bytes: Option<i64>,
	/// Bytes of decoded object payload sent to S3 during the restore.
	pub s3_sent_payload_bytes: Option<i64>,
	/// Bytes of raw HTTP traffic received from S3 during the restore,
	/// including protocol overhead.
	pub s3_received_raw_bytes: Option<i64>,
	/// Bytes of decoded object payload received from S3 during the restore.
	pub s3_received_payload_bytes: Option<i64>,
	/// Arbitrary structured health data to record alongside the report
	/// (database statistics, whether indexes needed rebuilding, and so on).
	/// Stored and displayed as-is.
	pub health_details: Option<serde_json::Value>,
	/// This must be the run-uuid the client minted for this run.
	/// The field is optional only so older clients don't break; it WILL be made
	/// mandatory in future.
	pub run_id: Option<Uuid>,
}

/// Report the outcome of a restore attempt and the replica's health.
///
/// Records a verification report for a restore the device performed from its
/// worklist. Send one report per attempt, on success and on failure alike. A
/// report with a `success` outcome and `replica_healthy: true` marks the
/// snapshot as verified; for run-once intents this is what removes the entry
/// from `GET /restore-worklist` until a newer snapshot appears.
///
/// Authorization matches `POST /restore-credentials`: the device must hold an
/// enabled restore declaration covering the reported group and type,
/// otherwise the request is rejected with 403.
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
			run_id: args.run_id,
		},
	)
	.await?;

	Ok(StatusCode::NO_CONTENT)
}
