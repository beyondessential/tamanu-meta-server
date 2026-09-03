//! S3-metrics task. Per tick (own slow cadence), read CloudWatch
//! `AWS/S3 BucketSizeBytes` for each `ready` group's bucket and store it in
//! `backup_repo_stats.bucket_bytes` (the billing basis). Best-effort: on any
//! error, log + continue, never alert.
//!
//! The metric lives in the *group's* account, so we read it with the group's
//! assumed maintenance role (`maintenance_role_arn` — the device role has no
//! CloudWatch grant), mirroring the assume→creds→client-builder pattern in
//! [`super::preflight`].

use std::{
	collections::{BTreeSet, HashMap},
	time::Duration,
};

use aws_sdk_cloudwatch::types::{Dimension, DimensionFilter, Statistic};
use commons_servers::backup_jobs::slot_deadline_due;
use database::{BackupConfigStatus, BackupRepoStats, ServerGroupBackupConfig};
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, warn};
use uuid::Uuid;

struct Aws {
	sts: aws_sdk_sts::Client,
	config: aws_config::SdkConfig,
}

const TICK: Duration = Duration::from_secs(60);
const METRICS_WINDOW: Duration = Duration::from_secs(24 * 3600);
const LOOKBACK_SECS: i64 = 3 * 24 * 3600; // BucketSizeBytes is daily; look back a few days.

/// Latest `BucketSizeBytes` average for a bucket, or `None` if the metric is
/// absent (best-effort).
async fn bucket_bytes(
	aws: &Aws,
	cfg: &ServerGroupBackupConfig,
	now: Timestamp,
) -> Result<Option<i64>, String> {
	// The metric is in the group's account; assume the group's maintenance
	// role to read it (the CloudWatch grant lives there, not on the device role).
	let resp = aws
		.sts
		.assume_role()
		.role_arn(&cfg.maintenance_role_arn)
		.role_session_name("canopy-s3-metrics")
		.send()
		.await
		.map_err(|e| format!("AssumeRole failed: {e}"))?;
	let c = resp
		.credentials()
		.ok_or_else(|| "AssumeRole returned no credentials".to_string())?;
	let creds = aws_sdk_cloudwatch::config::Credentials::new(
		c.access_key_id(),
		c.secret_access_key(),
		Some(c.session_token().to_string()),
		None,
		"canopy-s3-metrics",
	);
	let mut b = aws_sdk_cloudwatch::config::Builder::from(&aws.config).credentials_provider(creds);
	if let Some(region) = &cfg.region {
		b = b.region(aws_sdk_cloudwatch::config::Region::new(region.clone()));
	}
	let cw = aws_sdk_cloudwatch::Client::from_conf(b.build());

	let end = aws_sdk_cloudwatch::primitives::DateTime::from_secs(now.as_second());
	let start =
		aws_sdk_cloudwatch::primitives::DateTime::from_secs(now.as_second() - LOOKBACK_SECS);

	// `BucketSizeBytes` is reported per `StorageType` (storage class), and there
	// is no "all storage types" total for it. Which classes a bucket reports
	// depends on its config — Standard, Intelligent-Tiering tiers
	// (`IntelligentTieringFAStorage`/`IAStorage`/archive tiers), etc. — so
	// discover whichever StorageTypes this bucket actually emits via ListMetrics,
	// then sum the latest datapoint across them rather than assuming a class.
	let listed = cw
		.list_metrics()
		.namespace("AWS/S3")
		.metric_name("BucketSizeBytes")
		.dimensions(
			DimensionFilter::builder()
				.name("BucketName")
				.value(&cfg.bucket)
				.build(),
		)
		.send()
		.await
		.map_err(|e| e.to_string())?;
	let storage_types: BTreeSet<String> = listed
		.metrics()
		.iter()
		.filter_map(|m| {
			m.dimensions()
				.iter()
				.find(|d| d.name() == Some("StorageType"))
				.and_then(|d| d.value())
				.map(String::from)
		})
		.collect();

	let mut total: Option<i64> = None;
	for st in &storage_types {
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
			.dimensions(Dimension::builder().name("StorageType").value(st).build())
			.start_time(start)
			.end_time(end)
			.period(86400)
			.statistics(Statistic::Average)
			.send()
			.await
			.map_err(|e| e.to_string())?;
		// Most recent datapoint's average for this storage type.
		if let Some(v) = resp
			.datapoints()
			.iter()
			.filter_map(|d| d.timestamp().map(|t| (t.secs(), d.average())))
			.max_by_key(|(secs, _)| *secs)
			.and_then(|(_, avg)| avg)
		{
			total = Some(total.unwrap_or(0) + v as i64);
		}
	}
	Ok(total)
}

async fn tick(
	db: &mut diesel_async::AsyncPgConnection,
	aws: &Aws,
	last_run: &mut HashMap<Uuid, Timestamp>,
) -> Result<(), String> {
	let ready: Vec<ServerGroupBackupConfig> = ServerGroupBackupConfig::list(db)
		.await
		.map_err(|e| e.to_string())?
		.into_iter()
		.filter(|c| c.status == BackupConfigStatus::Ready)
		.collect();
	let now = Timestamp::now();

	for c in &ready {
		// Fire once per day at the group's jittered deadline, catching up on a
		// later tick if this one drifts past the slot or a slow predecessor group
		// pushed us late. The anchor is in-memory (`bucket_bytes_observed_at` is
		// only persisted on success): it resets on restart, re-firing at the next
		// deadline. Recorded on attempt, not success, so a persistently-failing
		// group doesn't hammer CloudWatch every tick.
		if !slot_deadline_due(
			c.group_id,
			METRICS_WINDOW,
			last_run.get(&c.group_id).copied(),
			now,
		) {
			continue;
		}
		last_run.insert(c.group_id, now);
		match bucket_bytes(aws, c, now).await {
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
		let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
		let aws = Aws {
			sts: aws_sdk_sts::Client::new(&config),
			config,
		};
		let mut last_run: HashMap<Uuid, Timestamp> = HashMap::new();
		loop {
			sleep(TICK).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};
			if let Err(e) = tick(&mut db, &aws, &mut last_run).await {
				error!("s3-metrics tick failed: {e}");
			}
		}
	})
}
