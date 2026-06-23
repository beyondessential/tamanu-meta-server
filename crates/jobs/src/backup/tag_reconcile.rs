//! Billing-tag reconcile. Per tick (slow, slot-jittered daily cadence), re-apply
//! each `ready` group's `billing.*` tags to its bucket when they've drifted —
//! catching group renames / rank changes (the bucket *name* can't follow a
//! rename, but `billing.deployment` does). Covers both placements: shared buckets
//! via the provisioner role, external (BYO) buckets via the group's maintenance
//! role. Best-effort: on any error (e.g. an external account whose maintenance
//! role doesn't yet grant `PutBucketTagging`), log + continue, never alert.

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
		match super::provision::reconcile_bucket_tags(&role_arn, &c.bucket, region, &tags).await {
			Ok(true) => debug!(group = %c.group_id, "tag-reconcile: applied billing tags"),
			Ok(false) => {}
			// Best-effort: an external role without PutBucketTagging lands here.
			Err(e) => warn!(group = %c.group_id, "tag-reconcile failed (non-fatal): {e:#}"),
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
