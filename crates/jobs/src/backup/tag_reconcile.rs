//! Bucket reconcile. Per tick (slow, slot-jittered daily cadence), converge each
//! `ready` group's bucket toward what canopy wants — so config changes reach
//! already-provisioned buckets, not just newly-created ones:
//!
//! - **Shared** (canopy-managed): re-apply the whole idempotent provisioning
//!   recipe via the provisioner role (object-lock, versioning, lifecycle incl.
//!   delete-marker reaping, the TLS-only policy, and the `billing.*` tags).
//! - **External** (BYO): reconcile only the `billing.*` tags via the group's
//!   maintenance role — canopy never owns a BYO bucket's own config.
//!
//! Tag drift catches group renames / rank changes (the bucket *name* can't follow
//! a rename, but `billing.deployment` does). Best-effort: on any error (e.g. an
//! external account whose maintenance role doesn't yet grant `PutBucketTagging`),
//! log + continue, never alert.

use std::time::Duration;

use commons_servers::backup_jobs::{backup_bucket_billing_tags, slot_is_due};
use commons_types::backup::BackupPlacement;
use database::{BackupConfigStatus, ServerGroupBackupConfig, server_groups::ServerGroup};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, warn};

const TICK: Duration = Duration::from_secs(60);
const RECONCILE_WINDOW: Duration = Duration::from_secs(24 * 3600);

fn secs_into(now: Timestamp, window: Duration) -> u64 {
	let w = window.as_secs().max(1) as i64;
	now.as_second().rem_euclid(w) as u64
}

async fn tick(db: &mut diesel_async::AsyncPgConnection) -> Result<(), String> {
	let ready: Vec<ServerGroupBackupConfig> = ServerGroupBackupConfig::list(db)
		.await
		.map_err(|e| e.to_string())?
		.into_iter()
		.filter(|c| c.status == BackupConfigStatus::Ready)
		.collect();
	if ready.is_empty() {
		return Ok(());
	}
	let now = Timestamp::now();
	let into = secs_into(now, RECONCILE_WINDOW);

	let group_ids: Vec<_> = ready.iter().map(|c| c.group_id).collect();
	let ranks = ServerGroup::highest_member_ranks(db, &group_ids)
		.await
		.map_err(|e| e.to_string())?;
	// Provisioner role for shared buckets; absent ⇒ skip shared reconciles.
	let provisioner = std::env::var("CANOPY_SHARED_BACKUP_PROVISIONER_ROLE_ARN")
		.ok()
		.filter(|v| !v.trim().is_empty());

	for c in &ready {
		if !slot_is_due(c.group_id, RECONCILE_WINDOW, TICK, into) {
			continue;
		}
		let group = match ServerGroup::get_by_id(db, c.group_id).await {
			Ok(g) => g,
			Err(e) => {
				warn!(group = %c.group_id, "tag-reconcile: group lookup failed: {e}");
				continue;
			}
		};
		let role_arn = match c.placement {
			BackupPlacement::Shared => match &provisioner {
				Some(arn) => arn.clone(),
				None => {
					warn!(group = %c.group_id, "tag-reconcile: shared config but CANOPY_SHARED_BACKUP_PROVISIONER_ROLE_ARN unset; skipping");
					continue;
				}
			},
			BackupPlacement::External => c.maintenance_role_arn.clone(),
		};
		let tags =
			backup_bucket_billing_tags(&group.tags, &group.name, ranks.get(&c.group_id).copied());
		let region = c.region.as_deref().unwrap_or("us-east-1");
		let result = match c.placement {
			// Shared buckets are canopy-managed: re-apply the whole (idempotent)
			// provisioning recipe so changes to it — object-lock, lifecycle (incl.
			// delete-marker reaping), the TLS-only policy, billing tags — converge
			// on already-provisioned buckets, not just newly-created ones.
			BackupPlacement::Shared => {
				super::provision::ensure_bucket(&role_arn, &c.bucket, region, &tags)
					.await
					.map(|()| "reconciled shared bucket")
			}
			// BYO buckets: canopy owns only the billing tags, never the bucket's
			// own config — reconcile tags alone, via the group's maintenance role.
			BackupPlacement::External => {
				super::provision::reconcile_bucket_tags(&role_arn, &c.bucket, region, &tags)
					.await
					.map(|changed| if changed { "applied billing tags" } else { "" })
			}
		};
		match result {
			Ok("") => {}
			Ok(msg) => debug!(group = %c.group_id, "bucket-reconcile: {msg}"),
			// Best-effort: e.g. an external role without PutBucketTagging lands here.
			Err(e) => warn!(group = %c.group_id, "bucket-reconcile failed (non-fatal): {e:#}"),
		}
	}
	Ok(())
}

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		loop {
			sleep(TICK).await;
			let Ok(mut db) = pool.get().await else {
				error!("tag-reconcile: failed to get database connection");
				continue;
			};
			if let Err(e) = tick(&mut db).await {
				error!("tag-reconcile tick failed: {e}");
			}
		}
	})
}
