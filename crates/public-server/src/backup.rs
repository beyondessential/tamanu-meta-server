//! Device backup endpoints — the on-demand credential-minting path of the
//! backup-credentials system.
//!
//! Four `ServerDevice`-authenticated endpoints, all mounted at the root:
//!
//! - `POST /backup-capabilities` — bestool registers the backup types it can
//!   run on this server.
//! - `POST /backup-credentials` — mint short-lived per-group S3 creds via a
//!   cross-account `sts:AssumeRole`, returned as `credential_process` JSON.
//! - `GET  /backup-target` — `{storage, bucket, prefix, region, repo_password}`
//!   so bestool can reconstruct the kopia repo connection on every run.
//! - `POST /backup-report` — record a run outcome into `backup_runs`.
//!
//! All four resolve `device → live server → group_id → group backup config`
//! identically: **412** when the device is bound to no live server, **409**
//! when the server is ungrouped / has no `ready` config / the type isn't an
//! enabled capability, **502** when STS or kube fails or isn't configured.

use aws_sdk_sts::operation::RequestId as _;
use axum::{Json, extract::State, http::StatusCode};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::ServerDevice;
use commons_types::backup::{BackupConfigStatus, BackupPurpose, BackupType, RunOutcome};
use database::{
	Db,
	backups::BackupTypeDefault,
	backups::{
		NewBackupCredentialIssuance, NewBackupRun, ServerBackupCapability, ServerGroupBackupConfig,
	},
	servers::Server,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::state::{AppState, BackupSecrets};

/// The data key inside the repo-password k8s Secret. The onboarding/escrow
/// component that *creates* the Secret must store the kopia passphrase under
/// this key. Single source of truth for the key name.
pub const REPO_PASSWORD_SECRET_KEY: &str = "password";

/// Fallback AWS region served by `GET /backup-target` when the group config's
/// `region` is NULL. Read from `AWS_REGION` (the EKS pod always has it), with a
/// last-resort default so the endpoint always returns a concrete region string.
fn deployment_default_region() -> String {
	std::env::var("AWS_REGION")
		.or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
		.unwrap_or_else(|_| "us-east-1".to_string())
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(capabilities))
		.routes(routes!(credentials))
		.routes(routes!(target))
		.routes(routes!(report))
}

// ---------------------------------------------------------------------------
// Shared resolution: device → live server → group_id → ready config
// ---------------------------------------------------------------------------

/// Resolve the authenticated device to its single live server.
/// Empty ⇒ 412 (`DeviceHasNoServer`).
async fn resolve_server(
	conn: &mut database::diesel_async::AsyncPgConnection,
	device_id: Uuid,
) -> Result<Server> {
	Server::live_by_device_id(conn, device_id)
		.await?
		.into_iter()
		.next()
		.ok_or(AppError::DeviceHasNoServer)
}

/// Read the server's `group_id`; ungrouped ⇒ 409.
fn require_group(server: &Server) -> Result<Uuid> {
	server
		.group_id
		.ok_or_else(|| AppError::Conflict("server is not in a group".into()))
}

/// Load the group's backup config and require it be `ready`. Absent or
/// non-`ready` ⇒ 409 (dormant gate).
async fn require_ready_config(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group_id: Uuid,
) -> Result<ServerGroupBackupConfig> {
	let cfg = ServerGroupBackupConfig::get(conn, group_id)
		.await?
		.ok_or_else(|| AppError::Conflict("group has no backup config".into()))?;
	if cfg.status != BackupConfigStatus::Ready {
		return Err(AppError::Conflict(
			"group backup config is not ready".into(),
		));
	}
	Ok(cfg)
}

/// Require that `(server, type)` is an enabled capability — the per-`(server,
/// type)` issuance gate. Not enabled / not registered ⇒ 409.
async fn require_enabled_capability(
	conn: &mut database::diesel_async::AsyncPgConnection,
	server_id: Uuid,
	r#type: &BackupType,
) -> Result<()> {
	let enabled = ServerBackupCapability::list_for_server(conn, server_id)
		.await?
		.into_iter()
		.any(|c| &c.r#type == r#type && c.enabled);
	if enabled {
		Ok(())
	} else {
		Err(AppError::Conflict(format!(
			"backup type {type} is not an enabled capability for this server",
			type = r#type
		)))
	}
}

// ---------------------------------------------------------------------------
// POST /backup-capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct CapabilitiesArgs {
	/// The backup types bestool can run on this server.
	#[schema(value_type = Vec<String>)]
	pub types: Vec<BackupType>,
}

#[utoipa::path(
	post,
	path = "/backup-capabilities",
	tag = "backup",
	security(("server-device" = [])),
	request_body = CapabilitiesArgs,
	responses(
		(status = 204, description = "Capabilities registered."),
		(status = 412, description = "Device is not bound to a live server.", body = ProblemDetailsSchema),
		(status = 409, description = "Server is not in a group.", body = ProblemDetailsSchema),
	),
)]
async fn capabilities(
	State(db): State<Db>,
	device: ServerDevice,
	Json(args): Json<CapabilitiesArgs>,
) -> Result<StatusCode> {
	let mut conn = db.get().await?;
	let device_id = device.0.0.id;

	let server = resolve_server(&mut conn, device_id).await?;
	// A capability is per-server; the group is required so the server is a
	// real, grouped member (matches the other endpoints' 409 on ungrouped).
	require_group(&server)?;

	for r#type in &args.types {
		// Seed `enabled` from the type's auto_enable default for newly-seen
		// types; existing rows keep their operator-set `enabled`.
		let seed = BackupTypeDefault::get(&mut conn, r#type)
			.await?
			.map(|d| d.auto_enable)
			.unwrap_or(false);
		ServerBackupCapability::register(&mut conn, server.id, r#type, seed).await?;
	}

	Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// POST /backup-credentials
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct CredentialsArgs {
	/// The backup type these creds are for. Must be an enabled capability.
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// `backup` (default) grants write-without-delete; `restore` is downscoped
	/// read-only via a session policy.
	#[serde(default)]
	pub purpose: BackupPurpose,
}

/// `credential_process` output. Field names are **fixed by the AWS SDK**:
/// `Version/AccessKeyId/SecretAccessKey/SessionToken/Expiration` — exactly what
/// `rename_all = "PascalCase"` produces from these field names.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "PascalCase")]
pub struct CredentialProcessOutput {
	/// Always the literal `1`.
	pub version: u8,
	pub access_key_id: String,
	pub secret_access_key: String,
	pub session_token: String,
	/// RFC3339 / ISO8601 `Z` instant.
	pub expiration: String,
}

/// Build the read-only **restore** session policy for a bucket/prefix. ANDs
/// down to read-only against the per-bucket role. `GetBucketLocation` is its
/// own unconditioned statement (the `s3:prefix` context key isn't populated for
/// it, so folding it under the prefix condition would silently deny it).
pub fn restore_session_policy(bucket: &str, prefix: &str) -> String {
	serde_json::json!({
		"Version": "2012-10-17",
		"Statement": [
			{
				"Effect": "Allow",
				"Action": ["s3:GetObject"],
				"Resource": format!("arn:aws:s3:::{bucket}/{prefix}*"),
			},
			{
				"Effect": "Allow",
				"Action": ["s3:GetBucketLocation"],
				"Resource": format!("arn:aws:s3:::{bucket}"),
			},
			{
				"Effect": "Allow",
				"Action": ["s3:ListBucket"],
				"Resource": format!("arn:aws:s3:::{bucket}"),
				"Condition": { "StringLike": { "s3:prefix": [format!("{prefix}*")] } },
			},
		],
	})
	.to_string()
}

#[utoipa::path(
	post,
	path = "/backup-credentials",
	tag = "backup",
	security(("server-device" = [])),
	request_body = CredentialsArgs,
	responses(
		(status = 200, body = CredentialProcessOutput),
		(status = 409, description = "Server ungrouped, no ready config, or type not enabled.", body = ProblemDetailsSchema),
		(status = 412, description = "Device is not bound to a live server.", body = ProblemDetailsSchema),
		(status = 502, description = "STS issuance failed or is not configured.", body = ProblemDetailsSchema),
	),
)]
async fn credentials(
	State(db): State<Db>,
	State(sts): State<Option<aws_sdk_sts::Client>>,
	device: ServerDevice,
	Json(args): Json<CredentialsArgs>,
) -> Result<Json<CredentialProcessOutput>> {
	let mut conn = db.get().await?;
	let device_id = device.0.0.id;

	let server = resolve_server(&mut conn, device_id).await?;
	let group_id = require_group(&server)?;
	let cfg = require_ready_config(&mut conn, group_id).await?;
	require_enabled_capability(&mut conn, server.id, &args.r#type).await?;

	let restore_policy = match args.purpose {
		BackupPurpose::Restore => Some(restore_session_policy(&cfg.bucket, &cfg.prefix)),
		BackupPurpose::Backup => None,
	};

	let Some(sts) = sts else {
		tracing::error!(group = %group_id, "backup-credentials: STS client not configured");
		return Err(AppError::Upstream(
			"credential issuer not configured".into(),
		));
	};

	let session_name = format!("canopy-{purpose}-{device_id}", purpose = args.purpose);

	let resp = sts
		.assume_role()
		.role_arn(&cfg.target_role_arn)
		.role_session_name(session_name)
		.set_policy(restore_policy)
		// Chained sessions cap at 1h regardless; ask for the max.
		.duration_seconds(3600)
		.send()
		.await
		.map_err(|err| {
			let request_id = err.request_id().unwrap_or("<none>");
			tracing::error!(
				group = %group_id,
				role = %cfg.target_role_arn,
				request_id,
				error = ?err,
				"backup-credentials: AssumeRole failed",
			);
			AppError::Upstream("credential issuance failed".into())
		})?;

	let sts_request_id = resp.request_id().map(str::to_owned);

	let creds = resp.credentials().ok_or_else(|| {
		tracing::error!(group = %group_id, "backup-credentials: AssumeRole returned no credentials");
		AppError::Upstream("credential issuance returned no credentials".into())
	})?;

	// STS `Expiration` is a smithy DateTime → convert to a jiff Timestamp for
	// the audit row, and emit RFC3339 `Z` for the credential_process output.
	let expiry_secs = creds.expiration().secs();
	let expires_at = Timestamp::from_second(expiry_secs).map_err(|err| {
		tracing::error!(group = %group_id, error = ?err, "backup-credentials: bad expiration");
		AppError::Upstream("credential issuance returned an invalid expiration".into())
	})?;

	let access_key_id = creds.access_key_id().to_owned();

	// Audit insert BEFORE returning — never hand out creds we didn't record.
	database::backups::BackupCredentialIssuance::record(
		&mut conn,
		NewBackupCredentialIssuance {
			device_id,
			group_id,
			r#type: args.r#type.clone(),
			expires_at,
			purpose: args.purpose,
			sts_assumed_role: cfg.target_role_arn.clone(),
			sts_request_id,
			access_key_id: Some(access_key_id.clone()),
			bucket: cfg.bucket.clone(),
			prefix: cfg.prefix.clone(),
		},
	)
	.await?;

	Ok(Json(CredentialProcessOutput {
		version: 1,
		access_key_id,
		secret_access_key: creds.secret_access_key().to_owned(),
		session_token: creds.session_token().to_owned(),
		expiration: expires_at.to_string(),
	}))
}

// ---------------------------------------------------------------------------
// GET /backup-target
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct BackupTarget {
	/// Always `"s3"`.
	pub storage: String,
	pub bucket: String,
	/// Normally empty (the repo lives at the bucket root).
	pub prefix: String,
	/// The group config's region, or the deployment default.
	pub region: String,
	/// The kopia repo passphrase, read from the group's k8s Secret.
	pub repo_password: String,
}

#[utoipa::path(
	get,
	path = "/backup-target",
	tag = "backup",
	security(("server-device" = [])),
	responses(
		(status = 200, body = BackupTarget),
		(status = 409, description = "Server ungrouped or no ready config.", body = ProblemDetailsSchema),
		(status = 412, description = "Device is not bound to a live server.", body = ProblemDetailsSchema),
		(status = 502, description = "Repo-password Secret unavailable or kube not configured.", body = ProblemDetailsSchema),
	),
)]
async fn target(
	State(db): State<Db>,
	State(kube): State<Option<BackupSecrets>>,
	device: ServerDevice,
) -> Result<Json<BackupTarget>> {
	let mut conn = db.get().await?;
	let device_id = device.0.0.id;

	let server = resolve_server(&mut conn, device_id).await?;
	let group_id = require_group(&server)?;
	let cfg = require_ready_config(&mut conn, group_id).await?;

	let Some(kube) = kube else {
		tracing::error!(group = %group_id, "backup-target: kube client not configured");
		return Err(AppError::Upstream("secret store not configured".into()));
	};

	let repo_password = kube
		.read_password(&cfg.repo_password_ref, REPO_PASSWORD_SECRET_KEY)
		.await
		.map_err(|err| {
			tracing::error!(
				group = %group_id,
				secret = %cfg.repo_password_ref,
				error = ?err,
				"backup-target: reading repo-password Secret failed",
			);
			AppError::Upstream("repo password unavailable".into())
		})?;

	let region = cfg.region.clone().unwrap_or_else(deployment_default_region);

	Ok(Json(BackupTarget {
		storage: "s3".into(),
		bucket: cfg.bucket,
		prefix: cfg.prefix,
		region,
		repo_password,
	}))
}

// ---------------------------------------------------------------------------
// POST /backup-report
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReportArgs {
	/// The run-uuid bestool minted at run start (becomes `backup_runs.id`).
	pub run_id: Uuid,
	/// The backup type that ran.
	#[schema(value_type = String)]
	pub r#type: BackupType,
	pub purpose: BackupPurpose,
	pub outcome: RunOutcome,
	pub error: Option<String>,
	pub bytes_uploaded: Option<i64>,
	pub snapshot_id: Option<String>,
}

#[utoipa::path(
	post,
	path = "/backup-report",
	tag = "backup",
	security(("server-device" = [])),
	request_body = ReportArgs,
	responses(
		(status = 204, description = "Run recorded."),
		(status = 409, description = "Server ungrouped, or duplicate run_id.", body = ProblemDetailsSchema),
		(status = 412, description = "Device is not bound to a live server.", body = ProblemDetailsSchema),
	),
)]
async fn report(
	State(db): State<Db>,
	device: ServerDevice,
	Json(rep): Json<ReportArgs>,
) -> Result<StatusCode> {
	let mut conn = db.get().await?;
	let device_id = device.0.0.id;

	let server = resolve_server(&mut conn, device_id).await?;
	// A report need not be `ready`, but the server must be grouped:
	// device_id/group_id come from the authenticated context, never the body.
	let group_id = require_group(&server)?;

	// `record` maps a PK (duplicate run_id) violation to AppError::Conflict (409).
	database::backups::BackupRun::record(
		&mut conn,
		NewBackupRun {
			id: rep.run_id,
			device_id,
			group_id,
			server_id: Some(server.id),
			r#type: rep.r#type,
			purpose: rep.purpose,
			outcome: rep.outcome,
			error: rep.error,
			bytes_uploaded: rep.bytes_uploaded,
			snapshot_id: rep.snapshot_id,
		},
	)
	.await?;

	Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn restore_policy_has_three_statements_and_prefix_substitution() {
		let policy = restore_session_policy("my-bucket", "");
		let v: serde_json::Value = serde_json::from_str(&policy).unwrap();

		assert_eq!(v["Version"], "2012-10-17");
		let stmts = v["Statement"].as_array().unwrap();
		assert_eq!(stmts.len(), 3);

		// 1: GetObject scoped to the prefix.
		assert_eq!(stmts[0]["Action"], serde_json::json!(["s3:GetObject"]));
		assert_eq!(stmts[0]["Resource"], "arn:aws:s3:::my-bucket/*");
		assert!(stmts[0].get("Condition").is_none());

		// 2: GetBucketLocation, unconditioned, bucket-level resource.
		assert_eq!(
			stmts[1]["Action"],
			serde_json::json!(["s3:GetBucketLocation"])
		);
		assert_eq!(stmts[1]["Resource"], "arn:aws:s3:::my-bucket");
		assert!(
			stmts[1].get("Condition").is_none(),
			"GetBucketLocation must be unconditioned — the s3:prefix key isn't populated for it",
		);

		// 3: ListBucket conditioned on the prefix.
		assert_eq!(stmts[2]["Action"], serde_json::json!(["s3:ListBucket"]));
		assert_eq!(stmts[2]["Resource"], "arn:aws:s3:::my-bucket");
		assert_eq!(
			stmts[2]["Condition"]["StringLike"]["s3:prefix"],
			serde_json::json!(["*"]),
		);

		// No mutation actions anywhere.
		let blob = policy.to_lowercase();
		for forbidden in ["putobject", "deleteobject", "abortmultipart"] {
			assert!(
				!blob.contains(forbidden),
				"restore policy must not grant {forbidden}"
			);
		}
	}

	#[test]
	fn restore_policy_substitutes_nonempty_prefix() {
		let policy = restore_session_policy("b", "repo/");
		let v: serde_json::Value = serde_json::from_str(&policy).unwrap();
		let stmts = v["Statement"].as_array().unwrap();
		assert_eq!(stmts[0]["Resource"], "arn:aws:s3:::b/repo/*");
		assert_eq!(
			stmts[2]["Condition"]["StringLike"]["s3:prefix"],
			serde_json::json!(["repo/*"]),
		);
	}

	#[test]
	fn credential_process_output_uses_exact_aws_casing() {
		let out = CredentialProcessOutput {
			version: 1,
			access_key_id: "AKIA".into(),
			secret_access_key: "secret".into(),
			session_token: "token".into(),
			expiration: "2026-05-21T13:00:00Z".into(),
		};
		let v: serde_json::Value = serde_json::to_value(&out).unwrap();
		assert_eq!(v["Version"], 1);
		assert_eq!(v["AccessKeyId"], "AKIA");
		assert_eq!(v["SecretAccessKey"], "secret");
		assert_eq!(v["SessionToken"], "token");
		assert_eq!(v["Expiration"], "2026-05-21T13:00:00Z");
	}
}
