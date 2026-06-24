//! Upstream-access preflight: watches Canopy's *own* access to each group's
//! bucket/role — not the devices — and alerts (never gates readiness).
//!
//! - Shared, every ~minute: `sts:GetCallerIdentity` (IRSA web-identity valid?).
//! - Per ready group, hourly (hash-jittered): assume the per-bucket role both
//!   ways (plain backup + read-only restore session policy), each followed by a
//!   read-only S3 no-op; and verify the bucket's Object-Lock is still ≥30-day
//!   GOVERNANCE.
//!
//! Alerts go through the group-level path (`database::backup::alerts`), which
//! bypasses per-server `is_monitored`. AWS calls are not unit-tested here (no
//! live AWS in CI); the pure logic — the restore session policy and the
//! Object-Lock assertion — is.

use std::time::Duration;

use aws_sdk_s3::types::ObjectLockRetentionMode;
use aws_sdk_sts::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_sts::operation::RequestId;
use commons_servers::backup_jobs::slot_is_due;
use commons_types::issue::Severity;
use database::{
	BackupConfigStatus, ServerGroupBackupConfig,
	backup::alerts::{raise_group_event, refs},
};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, warn};

const TICK: Duration = Duration::from_secs(60);
const DEEP_WINDOW: Duration = Duration::from_secs(3600);
const MIN_LOCK_DAYS: i32 = 30;

/// Build the read-only **restore** session policy (normative JSON from the
/// plan): `GetObject` on the prefix, an *unconditioned* `GetBucketLocation`
/// (the `s3:prefix` key isn't populated for it, so folding it under the prefix
/// condition would silently deny it), and a prefix-conditioned `ListBucket`.
fn restore_session_policy(bucket: &str, prefix: &str) -> String {
	let object_arn = format!("arn:aws:s3:::{bucket}/{prefix}*");
	let bucket_arn = format!("arn:aws:s3:::{bucket}");
	serde_json::json!({
		"Version": "2012-10-17",
		"Statement": [
			{ "Effect": "Allow", "Action": ["s3:GetObject"], "Resource": object_arn },
			{ "Effect": "Allow", "Action": ["s3:GetBucketLocation"], "Resource": bucket_arn },
			{
				"Effect": "Allow",
				"Action": ["s3:ListBucket"],
				"Resource": bucket_arn,
				"Condition": { "StringLike": { "s3:prefix": [format!("{prefix}*")] } }
			}
		]
	})
	.to_string()
}

/// Verdict of the Object-Lock check, given what `GetBucketObjectLockConfiguration`
/// returned. `Ok(())` only when an enabled GOVERNANCE (or COMPLIANCE) lock with
/// a default retention of ≥30 days is present.
fn validate_object_lock(
	mode: Option<ObjectLockRetentionMode>,
	days: Option<i32>,
) -> Result<(), String> {
	match (mode, days) {
		(Some(m), Some(d)) if d >= MIN_LOCK_DAYS => match m {
			ObjectLockRetentionMode::Governance | ObjectLockRetentionMode::Compliance => Ok(()),
			other => Err(format!("unexpected object-lock mode {other:?}")),
		},
		(Some(_), Some(d)) => Err(format!(
			"object-lock default retention {d}d < {MIN_LOCK_DAYS}d floor"
		)),
		_ => Err("object-lock configuration missing or has no default retention".to_string()),
	}
}

/// Seconds elapsed into the current `window` for `now` — used with
/// [`slot_is_due`] to fire a group's hourly deep check on the right minute tick.
fn secs_into_window(now: Timestamp, window: Duration) -> u64 {
	let w = window.as_secs().max(1) as i64;
	(now.as_second().rem_euclid(w)) as u64
}

struct Aws {
	sts: aws_sdk_sts::Client,
	config: aws_config::SdkConfig,
}

/// Human-readable detail for an AWS SDK error. The bare `SdkError` `Display` is
/// just "service error" (and the structured log shows nothing more), so pull out
/// the service error code + message + request id — that's what tells AccessDenied
/// from throttling from an expired token. Falls back to the `Display` chain for
/// non-service errors (timeouts, dispatch failures).
fn aws_detail<E, R>(e: &SdkError<E, R>) -> String
where
	E: ProvideErrorMetadata + std::error::Error,
	SdkError<E, R>: RequestId,
{
	let rid = e
		.request_id()
		.map(|r| format!(" (request id {r})"))
		.unwrap_or_default();
	match e.as_service_error() {
		Some(svc) => format!(
			"{}: {}{rid}",
			svc.code().unwrap_or("Unknown"),
			svc.message().unwrap_or("(no message)"),
		),
		None => format!("{e}{rid}"),
	}
}

async fn run_deep_check(aws: &Aws, cfg: &ServerGroupBackupConfig) -> Result<(), String> {
	// Both purposes must mint working creds + pass a read-only no-op.
	for restore in [false, true] {
		let leg = if restore { "restore" } else { "backup" };
		let mut assume = aws
			.sts
			.assume_role()
			.role_arn(&cfg.target_role_arn)
			.role_session_name(format!("canopy-preflight-{leg}"));
		if restore {
			assume = assume.policy(restore_session_policy(&cfg.bucket, &cfg.prefix));
		}
		let resp = assume
			.send()
			.await
			.map_err(|e| format!("{leg}: AssumeRole failed: {}", aws_detail(&e)))?;
		let c = resp
			.credentials()
			.ok_or_else(|| format!("{leg}: AssumeRole returned no credentials"))?;
		let creds = aws_sdk_s3::config::Credentials::new(
			c.access_key_id(),
			c.secret_access_key(),
			Some(c.session_token().to_string()),
			None,
			"canopy-preflight",
		);
		let mut s3b = aws_sdk_s3::config::Builder::from(&aws.config).credentials_provider(creds);
		if let Some(region) = &cfg.region {
			s3b = s3b.region(aws_sdk_s3::config::Region::new(region.clone()));
		}
		let s3 = aws_sdk_s3::Client::from_conf(s3b.build());

		// Read-only no-op proving the creds work for this leg.
		s3.get_bucket_location()
			.bucket(&cfg.bucket)
			.send()
			.await
			.map_err(|e| format!("{leg}: GetBucketLocation no-op failed: {}", aws_detail(&e)))?;
	}
	Ok(())
}

async fn check_object_lock(aws: &Aws, cfg: &ServerGroupBackupConfig) -> Result<(), String> {
	// Assume the (backup) role and read the lock config.
	let resp = aws
		.sts
		.assume_role()
		.role_arn(&cfg.target_role_arn)
		.role_session_name("canopy-preflight-lock")
		.send()
		.await
		.map_err(|e| {
			format!(
				"AssumeRole for object-lock check failed: {}",
				aws_detail(&e)
			)
		})?;
	let c = resp
		.credentials()
		.ok_or_else(|| "AssumeRole returned no credentials".to_string())?;
	let creds = aws_sdk_s3::config::Credentials::new(
		c.access_key_id(),
		c.secret_access_key(),
		Some(c.session_token().to_string()),
		None,
		"canopy-preflight",
	);
	let mut s3b = aws_sdk_s3::config::Builder::from(&aws.config).credentials_provider(creds);
	if let Some(region) = &cfg.region {
		s3b = s3b.region(aws_sdk_s3::config::Region::new(region.clone()));
	}
	let s3 = aws_sdk_s3::Client::from_conf(s3b.build());

	let resp = s3
		.get_object_lock_configuration()
		.bucket(&cfg.bucket)
		.send()
		.await
		.map_err(|e| {
			format!(
				"GetBucketObjectLockConfiguration failed: {}",
				aws_detail(&e)
			)
		})?;
	let rule = resp
		.object_lock_configuration()
		.and_then(|c| c.rule())
		.and_then(|r| r.default_retention());
	let mode = rule.and_then(|r| r.mode().cloned());
	let days = rule.and_then(|r| r.days());
	validate_object_lock(mode, days)
}

/// One full per-group deep pass: both-purpose issuance + object-lock, each
/// raising/recovering its own group-level alert.
async fn deep_check_group(
	db: &mut diesel_async::AsyncPgConnection,
	aws: &Aws,
	cfg: &ServerGroupBackupConfig,
) {
	match run_deep_check(aws, cfg).await {
		Ok(()) => {
			let _ = raise_group_event(
				db,
				cfg.group_id,
				refs::PREFLIGHT_ASSUME,
				Severity::Info,
				None,
				"issuance preflight ok",
				false,
			)
			.await;
		}
		Err(msg) => {
			error!(group = %cfg.group_id, "preflight assume failed: {msg}");
			let _ = raise_group_event(
				db,
				cfg.group_id,
				refs::PREFLIGHT_ASSUME,
				Severity::Error,
				Some("backup credential preflight failed"),
				&msg,
				true,
			)
			.await;
		}
	}
	match check_object_lock(aws, cfg).await {
		Ok(()) => {
			let _ = raise_group_event(
				db,
				cfg.group_id,
				refs::PREFLIGHT_OBJECT_LOCK,
				Severity::Info,
				None,
				"object lock ok",
				false,
			)
			.await;
		}
		Err(msg) => {
			error!(group = %cfg.group_id, "object-lock check failed: {msg}");
			let _ = raise_group_event(
				db,
				cfg.group_id,
				refs::PREFLIGHT_OBJECT_LOCK,
				Severity::Critical,
				Some("bucket Object-Lock missing or weakened"),
				&msg,
				true,
			)
			.await;
		}
	}
}

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
		let aws = Aws {
			sts: aws_sdk_sts::Client::new(&config),
			config,
		};

		loop {
			sleep(TICK).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};

			let ready: Vec<ServerGroupBackupConfig> =
				match ServerGroupBackupConfig::list(&mut db).await {
					Ok(all) => all
						.into_iter()
						.filter(|c| c.status == BackupConfigStatus::Ready)
						.collect(),
					Err(err) => {
						error!("preflight: failed to list backup configs: {err}");
						continue;
					}
				};

			// Shared check (every tick): IRSA identity resolves.
			let identity_ok = aws.sts.get_caller_identity().send().await;
			for cfg in &ready {
				match &identity_ok {
					Ok(_) => {
						let _ = raise_group_event(
							&mut db,
							cfg.group_id,
							refs::PREFLIGHT_IDENTITY,
							Severity::Info,
							None,
							"caller identity ok",
							false,
						)
						.await;
					}
					Err(e) => {
						let msg = format!("sts:GetCallerIdentity failed: {}", aws_detail(e));
						warn!("preflight: {msg}");
						let _ = raise_group_event(
							&mut db,
							cfg.group_id,
							refs::PREFLIGHT_IDENTITY,
							Severity::Critical,
							Some("Canopy IRSA identity broken"),
							&msg,
							true,
						)
						.await;
					}
				}
			}
			if identity_ok.is_err() {
				// No point hammering per-group assume when the shared identity is down.
				continue;
			}

			// Per-group deep checks on their hash-jittered hourly slot.
			let now = Timestamp::now();
			let into = secs_into_window(now, DEEP_WINDOW);
			for cfg in &ready {
				if slot_is_due(cfg.group_id, DEEP_WINDOW, TICK, into) {
					debug!(group = %cfg.group_id, "running hourly preflight deep check");
					deep_check_group(&mut db, &aws, cfg).await;
				}
			}
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn restore_policy_has_unconditioned_get_bucket_location() {
		let p = restore_session_policy("bes-kopia-backups-x", "");
		let v: serde_json::Value = serde_json::from_str(&p).unwrap();
		let stmts = v["Statement"].as_array().unwrap();
		// GetBucketLocation must be its own statement with no Condition.
		let gbl = stmts
			.iter()
			.find(|s| {
				s["Action"]
					.as_array()
					.unwrap()
					.iter()
					.any(|a| a == "s3:GetBucketLocation")
			})
			.expect("GetBucketLocation present");
		assert!(
			gbl.get("Condition").is_none(),
			"GetBucketLocation must be unconditioned"
		);
		// No mutation actions in the restore policy.
		assert!(!p.contains("PutObject") && !p.contains("DeleteObject"));
	}

	#[test]
	fn object_lock_validation() {
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), Some(30)).is_ok());
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), Some(35)).is_ok());
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), Some(7)).is_err());
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), None).is_err());
		assert!(validate_object_lock(None, None).is_err());
	}
}
