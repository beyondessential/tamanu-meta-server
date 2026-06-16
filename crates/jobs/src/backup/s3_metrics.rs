//! S3-metrics task (component 3). Per tick (own slow cadence), read CloudWatch
//! `AWS/S3 BucketSizeBytes` for each `ready` group's bucket and store it in
//! `backup_repo_stats.bucket_bytes` (the billing basis). Separate bin so it
//! carries CloudWatch permissions the read-only inspector deliberately
//! doesn't. Best-effort: on any error, log + continue, never alert.
//!
//! NOTE (flagged, spec §8 #6): the metric lives in the *deployment* account
//! (cross-account read) and the correct `StorageType` dimension for a
//! versioned+locked bucket needs empirical confirmation. This first cut uses a
//! per-group-region client with the pod's own creds and `StandardStorage`;
//! wiring the dedicated cross-account CloudWatch IRSA (or assume-the-group-role)
//! is the follow-up.

use std::time::Duration;

use aws_sdk_cloudwatch::types::{Dimension, Statistic};
use commons_servers::backup_jobs::slot_is_due;
use database::{BackupConfigStatus, BackupRepoStats, ServerGroupBackupConfig};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, warn};

const TICK: Duration = Duration::from_secs(60);
const METRICS_WINDOW: Duration = Duration::from_secs(24 * 3600);
const LOOKBACK_SECS: i64 = 3 * 24 * 3600; // BucketSizeBytes is daily; look back a few days.

fn secs_into(now: Timestamp, window: Duration) -> u64 {
	let w = window.as_secs().max(1) as i64;
	now.as_second().rem_euclid(w) as u64
}

/// Latest `BucketSizeBytes` average for a bucket, or `None` if the metric is
/// absent (best-effort).
async fn bucket_bytes(
	base: &aws_config::SdkConfig,
	cfg: &ServerGroupBackupConfig,
	now: Timestamp,
) -> Result<Option<i64>, String> {
	let mut b = aws_sdk_cloudwatch::config::Builder::from(base);
	if let Some(region) = &cfg.region {
		b = b.region(aws_sdk_cloudwatch::config::Region::new(region.clone()));
	}
	let cw = aws_sdk_cloudwatch::Client::from_conf(b.build());

	let end = aws_sdk_cloudwatch::primitives::DateTime::from_secs(now.as_second());
	let start =
		aws_sdk_cloudwatch::primitives::DateTime::from_secs(now.as_second() - LOOKBACK_SECS);

	let resp = cw
		.get_metric_statistics()
		.namespace("AWS/S3")
		.metric_name("BucketSizeBytes")
		.dimensions(
			Dimension::builder()
				.name("BucketName")
				.value(&cfg.bucket)
				.build(),
		)
		.dimensions(
			Dimension::builder()
				.name("StorageType")
				.value("StandardStorage")
				.build(),
		)
		.start_time(start)
		.end_time(end)
		.period(86400)
		.statistics(Statistic::Average)
		.send()
		.await
		.map_err(|e| e.to_string())?;

	// Take the most recent datapoint's average.
	let latest = resp
		.datapoints()
		.iter()
		.filter_map(|d| d.timestamp().map(|t| (t.secs(), d.average())))
		.max_by_key(|(secs, _)| *secs)
		.and_then(|(_, avg)| avg)
		.map(|avg| avg as i64);
	Ok(latest)
}

async fn tick(
	db: &mut diesel_async::AsyncPgConnection,
	base: &aws_config::SdkConfig,
) -> Result<(), String> {
	let ready: Vec<ServerGroupBackupConfig> = ServerGroupBackupConfig::list(db)
		.await
		.map_err(|e| e.to_string())?
		.into_iter()
		.filter(|c| c.status == BackupConfigStatus::Ready)
		.collect();
	let now = Timestamp::now();
	let into = secs_into(now, METRICS_WINDOW);

	for c in &ready {
		if !slot_is_due(c.group_id, METRICS_WINDOW, TICK, into) {
			continue;
		}
		match bucket_bytes(base, c, now).await {
			Ok(Some(bytes)) => {
				if let Err(e) =
					BackupRepoStats::upsert_bucket_bytes(db, c.group_id, Some(bytes)).await
				{
					warn!(group = %c.group_id, "s3-metrics: upsert failed: {e}");
				} else {
					debug!(group = %c.group_id, "s3-metrics: bucket_bytes = {bytes}");
				}
			}
			Ok(None) => debug!(group = %c.group_id, "s3-metrics: no BucketSizeBytes datapoint yet"),
			// Best-effort: never alert, just log.
			Err(e) => warn!(group = %c.group_id, "s3-metrics: CloudWatch read failed: {e}"),
		}
	}
	Ok(())
}

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		let base = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
		loop {
			sleep(TICK).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};
			if let Err(e) = tick(&mut db, &base).await {
				error!("s3-metrics tick failed: {e}");
			}
		}
	})
}
