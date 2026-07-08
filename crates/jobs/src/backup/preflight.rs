//! Upstream-access preflight: watches Canopy's *own* access to each group's
//! bucket/role — not the devices — and alerts (never gates readiness).
//!
//! - Shared, every ~minute: `sts:GetCallerIdentity` (IRSA web-identity valid?).
//! - Per ready group, hourly (hash-jittered): assume the **maintenance** role
//!   (what the backups pod actually uses) followed by a read-only S3 no-op, and
//!   verify the bucket's Object-Lock is still ≥30-day GOVERNANCE.
//!
//! This runs as the pod's `canopy-jobs` identity, which the maintenance role
//! trusts but the **device** role deliberately does not (the device role is
//! reachable only from `canopy-issuer`/`canopy-private`). So the device-role
//! (issuance) path is validated at onboarding by private-server's probe, not
//! here; later drift surfaces as real device backups failing.
//!
//! Alerts go through `database::issues::file_canopy_check`, which
//! bypasses per-server `is_monitored`. AWS calls are not unit-tested here (no
//! live AWS in CI); the pure logic — the Object-Lock assertion — is.

use std::{collections::HashMap, time::Duration};

use aws_sdk_s3::types::ObjectLockRetentionMode;
use aws_sdk_sts::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_sts::operation::RequestId;
use commons_servers::backup_jobs::slot_deadline_due;
use commons_types::status::CheckResult;
use database::{
	BackupConfigStatus, ServerGroupBackupConfig,
	backup::refs,
	issues::{CanopyCheckFiling, FilingScope, file_canopy_check},
};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, warn};
use uuid::Uuid;

const TICK: Duration = Duration::from_secs(60);
const DEEP_WINDOW: Duration = Duration::from_secs(3600);
const MIN_LOCK_DAYS: i32 = 30;

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

/// Raise the shared-identity self-alert (registers as an escalating
/// failure → notifies immediately).
async fn file_identity_alert(
	db: &mut diesel_async::AsyncPgConnection,
	msg: &str,
) -> Result<(), commons_errors::AppError> {
	database::self_alerts::raise(
		db,
		refs::PREFLIGHT_IDENTITY,
		CheckResult::Failed,
		CheckResult::Failed,
		true,
		"Canopy IRSA identity broken",
		msg,
	)
	.await?;
	Ok(())
}

/// Recover the shared-identity alert wherever it is live: the self-alert,
/// and any group-scoped issues left over from when this alert fanned out per
/// group (without this, a legacy issue that was active at deploy time would
/// stay open forever). Writes nothing when nothing is active.
async fn recover_identity_alert(
	db: &mut diesel_async::AsyncPgConnection,
) -> Result<(), commons_errors::AppError> {
	use database::issues::Issue;

	database::self_alerts::recover(db, refs::PREFLIGHT_IDENTITY, "caller identity ok").await?;

	for group_id in
		Issue::active_group_ids_by_source_ref(db, refs::CANOPY_SOURCE, refs::PREFLIGHT_IDENTITY)
			.await?
	{
		file_canopy_check(
			db,
			CanopyCheckFiling {
				scope: FilingScope::Group(group_id),
				check: refs::PREFLIGHT_IDENTITY,
				observed: CheckResult::Passed,
				title: None,
				message: "caller identity ok",
				detail: None,
				default_ceiling: CheckResult::Failed,
				default_escalates: true,
			},
		)
		.await?;
	}
	Ok(())
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
	// Validate the pod's own (maintenance-role) access: assume it and pass a
	// read-only S3 no-op. The device/issuance role is validated at onboarding by
	// private-server — the pod's canopy-jobs identity can't assume it by design.
	let resp = aws
		.sts
		.assume_role()
		.role_arn(&cfg.maintenance_role_arn)
		.role_session_name("canopy-preflight-maintenance")
		.send()
		.await
		.map_err(|e| format!("AssumeRole (maintenance) failed: {}", aws_detail(&e)))?;
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

	// Read-only no-op proving the creds work.
	s3.get_bucket_location()
		.bucket(&cfg.bucket)
		.send()
		.await
		.map_err(|e| format!("GetBucketLocation no-op failed: {}", aws_detail(&e)))?;
	Ok(())
}

async fn check_object_lock(aws: &Aws, cfg: &ServerGroupBackupConfig) -> Result<(), String> {
	// Assume the maintenance role (the pod's identity can assume it; it has the
	// S3 read perms) and read the lock config.
	let resp = aws
		.sts
		.assume_role()
		.role_arn(&cfg.maintenance_role_arn)
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

/// One full per-group deep pass: maintenance-role access + object-lock, each
/// raising/recovering its own group-level alert.
async fn deep_check_group(
	db: &mut diesel_async::AsyncPgConnection,
	aws: &Aws,
	cfg: &ServerGroupBackupConfig,
) {
	match run_deep_check(aws, cfg).await {
		Ok(()) => {
			let _ = file_canopy_check(
				db,
				CanopyCheckFiling {
					scope: FilingScope::Group(cfg.group_id),
					check: refs::PREFLIGHT_ASSUME,
					observed: CheckResult::Passed,
					title: None,
					message: "maintenance-role access ok",
					detail: None,
					default_ceiling: CheckResult::Failed,
					default_escalates: false,
				},
			)
			.await;
		}
		Err(msg) => {
			error!(group = %cfg.group_id, "preflight assume failed: {msg}");
			let _ = file_canopy_check(
				db,
				CanopyCheckFiling {
					scope: FilingScope::Group(cfg.group_id),
					check: refs::PREFLIGHT_ASSUME,
					observed: CheckResult::Failed,
					title: Some("backup maintenance-role access failed"),
					message: &msg,
					detail: None,
					default_ceiling: CheckResult::Failed,
					default_escalates: false,
				},
			)
			.await;
		}
	}
	match check_object_lock(aws, cfg).await {
		Ok(()) => {
			let _ = file_canopy_check(
				db,
				CanopyCheckFiling {
					scope: FilingScope::Group(cfg.group_id),
					check: refs::PREFLIGHT_OBJECT_LOCK,
					observed: CheckResult::Passed,
					title: None,
					message: "object lock ok",
					detail: None,
					default_ceiling: CheckResult::Failed,
					default_escalates: true,
				},
			)
			.await;
		}
		Err(msg) => {
			error!(group = %cfg.group_id, "object-lock check failed: {msg}");
			let _ = file_canopy_check(
				db,
				CanopyCheckFiling {
					scope: FilingScope::Group(cfg.group_id),
					check: refs::PREFLIGHT_OBJECT_LOCK,
					observed: CheckResult::Failed,
					title: Some("bucket Object-Lock missing or weakened"),
					message: &msg,
					detail: None,
					default_ceiling: CheckResult::Failed,
					default_escalates: true,
				},
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
		// In-memory per-group anchor for the hourly deep check (no persisted
		// last-check timestamp): lets a missed slot catch up on a later tick and
		// keeps a slow predecessor group from pushing others past their slot.
		// Resets on restart, re-firing at the next deadline.
		let mut last_deep: HashMap<Uuid, Timestamp> = HashMap::new();

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

			// Shared check (every tick): IRSA identity resolves. Filed ONCE
			// as a canopy-wide self-alert — a broken shared identity is one
			// fact about canopy, not one per group, and paging N groups at
			// once for it helps nobody.
			let identity_ok = aws.sts.get_caller_identity().send().await;
			if !ready.is_empty() {
				match &identity_ok {
					Ok(_) => {
						if let Err(err) = recover_identity_alert(&mut db).await {
							error!("preflight: identity-alert recovery failed: {err}");
						}
					}
					Err(e) => {
						let msg = format!("sts:GetCallerIdentity failed: {}", aws_detail(e));
						warn!("preflight: {msg}");
						if let Err(err) = file_identity_alert(&mut db, &msg).await {
							error!("preflight: identity-alert filing failed: {err}");
						}
					}
				}
			}
			if identity_ok.is_err() {
				// No point hammering per-group assume when the shared identity is down.
				continue;
			}

			// Per-group deep checks on their hash-jittered hourly deadline, with
			// catch-up on a later tick if this one drifts past the slot.
			let now = Timestamp::now();
			for cfg in &ready {
				if slot_deadline_due(
					cfg.group_id,
					DEEP_WINDOW,
					last_deep.get(&cfg.group_id).copied(),
					now,
				) {
					last_deep.insert(cfg.group_id, now);
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
	fn object_lock_validation() {
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), Some(30)).is_ok());
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), Some(35)).is_ok());
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), Some(7)).is_err());
		assert!(validate_object_lock(Some(ObjectLockRetentionMode::Governance), None).is_err());
		assert!(validate_object_lock(None, None).is_err());
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn identity_alert_files_once_and_recovers_legacy_fanout() {
		use commons_types::issue::Severity;
		use database::issues::{Issue, raise_group_event};
		use diesel_async::SimpleAsyncConnection as _;
		use uuid::Uuid;

		commons_tests::db::TestDb::run(async |mut conn, _url| {
			// A leftover from the fan-out era: an active group-scoped
			// identity issue that predates this deploy.
			let group = Uuid::new_v4();
			conn.batch_execute(&format!(
				"INSERT INTO server_groups (id, name) VALUES ('{group}', 'Legacy');"
			))
			.await
			.expect("seed group");
			raise_group_event(
				&mut conn,
				group,
				refs::PREFLIGHT_IDENTITY,
				Severity::Critical,
				Some("Canopy IRSA identity broken"),
				"legacy fan-out alert",
				true,
			)
			.await
			.expect("seed legacy alert");

			// Filing twice coalesces into the one canopy-wide issue.
			file_identity_alert(&mut conn, "sts:GetCallerIdentity failed: boom")
				.await
				.expect("file");
			file_identity_alert(&mut conn, "sts:GetCallerIdentity failed: boom")
				.await
				.expect("file again");
			let global = database::issues::get_global_issue(&mut conn, refs::PREFLIGHT_IDENTITY)
				.await
				.expect("get")
				.expect("one coalescing canopy-wide issue");
			assert!(global.active);

			// Recovery clears the canopy-wide issue AND the legacy one.
			recover_identity_alert(&mut conn).await.expect("recover");
			let global = database::issues::get_global_issue(&mut conn, refs::PREFLIGHT_IDENTITY)
				.await
				.expect("get")
				.expect("issue still exists");
			assert!(!global.active, "canopy-wide alert recovered");
			assert!(
				Issue::active_group_ids_by_source_ref(
					&mut conn,
					refs::CANOPY_SOURCE,
					refs::PREFLIGHT_IDENTITY,
				)
				.await
				.expect("scan")
				.is_empty(),
				"legacy group-scoped alert recovered"
			);

			// Recovering again with nothing live is a no-op.
			recover_identity_alert(&mut conn)
				.await
				.expect("idle recover");
		})
		.await
	}
}
