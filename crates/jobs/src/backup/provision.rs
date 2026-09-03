//! Create + configure a kopia backup bucket in the shared account
//! (`placement=shared`).
//!
//! BYO (`external`) buckets are made by ops/pulumi in the group's own
//! account; for shared-account configs Canopy creates the bucket itself at init,
//! applying the same security recipe pulumi's `backups` stack uses — Object Lock
//! + default GOVERNANCE retention, versioning, a reclaim lifecycle,
//! public-access-block, a TLS-only policy — plus the billing tags. Idempotent:
//! re-running adopts the already-owned bucket and re-applies the (idempotent)
//! config, so a retried init is safe.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::operation::create_bucket::CreateBucketError;
use aws_sdk_s3::types::{
	AbortIncompleteMultipartUpload, BucketLifecycleConfiguration, BucketLocationConstraint,
	BucketVersioningStatus, CreateBucketConfiguration, DefaultRetention, ExpirationStatus,
	LifecycleExpiration, LifecycleRule, LifecycleRuleFilter, NoncurrentVersionExpiration,
	ObjectLockConfiguration, ObjectLockEnabled, ObjectLockRetentionMode, ObjectLockRule,
	PublicAccessBlockConfiguration, Tag, Tagging, VersioningConfiguration,
};

/// Default Object Lock retention applied bucket-wide (server-side on every PUT,
/// so device creds need no `PutObjectRetention`).
const DEFAULT_RETENTION_DAYS: i32 = 30;

/// Assume the provisioner role and ensure `bucket` exists and is fully
/// configured. `tags` is the billing tag set (see
/// [`commons_servers::backup_jobs::backup_bucket_billing_tags`]).
pub async fn ensure_bucket(
	provisioner_role_arn: &str,
	bucket: &str,
	region: &str,
	tags: &[(String, String)],
) -> Result<()> {
	let s3 = s3_client_for(provisioner_role_arn, region).await?;
	apply_recipe(&s3, bucket, region, tags).await
}

/// Re-apply `tags` (the billing tags) to an existing bucket, **preserving** any
/// non-billing tags, and only writing when they've actually drifted. Returns
/// whether a change was applied. `role_arn` is the provisioner role for shared
/// buckets, or the group's maintenance role for external (BYO) buckets. Used by
/// the [`super::tag_reconcile`] loop.
pub async fn reconcile_bucket_tags(
	role_arn: &str,
	bucket: &str,
	region: &str,
	tags: &[(String, String)],
) -> Result<bool> {
	let s3 = s3_client_for(role_arn, region).await?;
	reconcile_tags(&s3, bucket, tags).await
}

/// Assume `role_arn` and build an S3 client scoped to `region`.
async fn s3_client_for(role_arn: &str, region: &str) -> Result<aws_sdk_s3::Client> {
	let sdk = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
	let sts = aws_sdk_sts::Client::new(&sdk);
	let assumed = sts
		.assume_role()
		.role_arn(role_arn)
		.role_session_name("canopy-provision")
		.send()
		.await
		.context("assume role")?;
	let c = assumed
		.credentials()
		.context("AssumeRole returned no credentials")?;
	let creds = aws_sdk_s3::config::Credentials::new(
		c.access_key_id(),
		c.secret_access_key(),
		Some(c.session_token().to_string()),
		None,
		"canopy-provision",
	);
	Ok(aws_sdk_s3::Client::from_conf(
		aws_sdk_s3::config::Builder::from(&sdk)
			.credentials_provider(creds)
			.region(aws_sdk_s3::config::Region::new(region.to_string()))
			.build(),
	))
}

/// The idempotent create + config sequence. Separated from credential assembly
/// so it can be exercised against a mocked S3 client.
async fn apply_recipe(
	s3: &aws_sdk_s3::Client,
	bucket: &str,
	region: &str,
	tags: &[(String, String)],
) -> Result<()> {
	create_bucket(s3, bucket, region).await?;

	s3.put_bucket_versioning()
		.bucket(bucket)
		.versioning_configuration(
			VersioningConfiguration::builder()
				.status(BucketVersioningStatus::Enabled)
				.build(),
		)
		.send()
		.await
		.context("put_bucket_versioning")?;

	s3.put_object_lock_configuration()
		.bucket(bucket)
		.object_lock_configuration(
			ObjectLockConfiguration::builder()
				.object_lock_enabled(ObjectLockEnabled::Enabled)
				.rule(
					ObjectLockRule::builder()
						.default_retention(
							DefaultRetention::builder()
								.mode(ObjectLockRetentionMode::Governance)
								.days(DEFAULT_RETENTION_DAYS)
								.build(),
						)
						.build(),
				)
				.build(),
		)
		.send()
		.await
		.context("put_object_lock_configuration")?;

	s3.put_public_access_block()
		.bucket(bucket)
		.public_access_block_configuration(
			PublicAccessBlockConfiguration::builder()
				.block_public_acls(true)
				.ignore_public_acls(true)
				.block_public_policy(true)
				.restrict_public_buckets(true)
				.build(),
		)
		.send()
		.await
		.context("put_public_access_block")?;

	s3.put_bucket_lifecycle_configuration()
		.bucket(bucket)
		.lifecycle_configuration(lifecycle()?)
		.send()
		.await
		.context("put_bucket_lifecycle_configuration")?;

	s3.put_bucket_policy()
		.bucket(bucket)
		.policy(tls_only_policy(bucket))
		.send()
		.await
		.context("put_bucket_policy")?;

	let tag_set = tags
		.iter()
		.map(|(k, v)| Tag::builder().key(k).value(v).build())
		.collect::<std::result::Result<Vec<_>, _>>()
		.context("build tag")?;
	s3.put_bucket_tagging()
		.bucket(bucket)
		.tagging(Tagging::builder().set_tag_set(Some(tag_set)).build()?)
		.send()
		.await
		.context("put_bucket_tagging")?;

	Ok(())
}

/// `CreateBucket`, treating "already owned / already exists" as success
/// (idempotent retry). `us-east-1` must omit the `LocationConstraint`.
async fn create_bucket(s3: &aws_sdk_s3::Client, bucket: &str, region: &str) -> Result<()> {
	let mut req = s3
		.create_bucket()
		.bucket(bucket)
		.object_lock_enabled_for_bucket(true);
	if region != "us-east-1" {
		req = req.create_bucket_configuration(
			CreateBucketConfiguration::builder()
				.location_constraint(BucketLocationConstraint::from(region))
				.build(),
		);
	}
	match req.send().await {
		Ok(_) => Ok(()),
		Err(e)
			if matches!(
				e.as_service_error(),
				Some(CreateBucketError::BucketAlreadyOwnedByYou(_))
					| Some(CreateBucketError::BucketAlreadyExists(_))
			) =>
		{
			Ok(())
		}
		Err(e) => Err(e).context("create_bucket"),
	}
}

/// Reclaim lifecycle for the versioned, object-locked bucket: noncurrent
/// versions expire (lock governs the actual delete time), the delete-markers
/// kopia leaves once their noncurrent version is gone are reaped, and incomplete
/// multipart uploads are aborted.
fn lifecycle() -> Result<BucketLifecycleConfiguration> {
	let rule = LifecycleRule::builder()
		.id("canopy-reclaim")
		.status(ExpirationStatus::Enabled)
		.filter(LifecycleRuleFilter::builder().prefix("").build())
		.noncurrent_version_expiration(
			NoncurrentVersionExpiration::builder()
				.noncurrent_days(1)
				.build(),
		)
		// kopia deletes are version-less, so each leaves a delete-marker; once its
		// last noncurrent version expires, clean up the dangling marker too (else
		// they accumulate forever). Marker-only expiry — no Days/Date.
		.expiration(
			LifecycleExpiration::builder()
				.expired_object_delete_marker(true)
				.build(),
		)
		.abort_incomplete_multipart_upload(
			AbortIncompleteMultipartUpload::builder()
				.days_after_initiation(7)
				.build(),
		)
		.build()
		.context("build lifecycle rule")?;
	BucketLifecycleConfiguration::builder()
		.rules(rule)
		.build()
		.context("build lifecycle configuration")
}

/// Merge `desired` (the billing tags) over the bucket's current tags —
/// preserving any non-billing tags — and `PutBucketTagging` only if anything
/// changed. `PutBucketTagging` replaces the whole set, hence the read-merge.
async fn reconcile_tags(
	s3: &aws_sdk_s3::Client,
	bucket: &str,
	desired: &[(String, String)],
) -> Result<bool> {
	let mut merged: BTreeMap<String, String> =
		current_tags(s3, bucket).await?.into_iter().collect();
	let mut changed = false;
	for (k, v) in desired {
		if merged.get(k).map(String::as_str) != Some(v.as_str()) {
			merged.insert(k.clone(), v.clone());
			changed = true;
		}
	}
	if !changed {
		return Ok(false);
	}
	let tag_set = merged
		.iter()
		.map(|(k, v)| Tag::builder().key(k).value(v).build())
		.collect::<std::result::Result<Vec<_>, _>>()
		.context("build tag")?;
	s3.put_bucket_tagging()
		.bucket(bucket)
		.tagging(Tagging::builder().set_tag_set(Some(tag_set)).build()?)
		.send()
		.await
		.context("put_bucket_tagging")?;
	Ok(true)
}

/// Current bucket tags. An untagged bucket returns `NoSuchTagSet`, which is an
/// empty set, not an error.
async fn current_tags(s3: &aws_sdk_s3::Client, bucket: &str) -> Result<Vec<(String, String)>> {
	match s3.get_bucket_tagging().bucket(bucket).send().await {
		Ok(out) => Ok(out
			.tag_set()
			.iter()
			.map(|t| (t.key().to_string(), t.value().to_string()))
			.collect()),
		Err(e) if e.as_service_error().and_then(|se| se.code()) == Some("NoSuchTagSet") => {
			Ok(Vec::new())
		}
		Err(e) => Err(e).context("get_bucket_tagging"),
	}
}

/// Deny any non-TLS access to the bucket.
fn tls_only_policy(bucket: &str) -> String {
	serde_json::json!({
		"Version": "2012-10-17",
		"Statement": [{
			"Sid": "DenyInsecureTransport",
			"Effect": "Deny",
			"Principal": "*",
			"Action": "s3:*",
			"Resource": [
				format!("arn:aws:s3:::{bucket}"),
				format!("arn:aws:s3:::{bucket}/*"),
			],
			"Condition": { "Bool": { "aws:SecureTransport": "false" } }
		}]
	})
	.to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tls_only_policy_denies_insecure_transport_for_bucket_and_objects() {
		let p: serde_json::Value =
			serde_json::from_str(&tls_only_policy("bes-canopy-backup-x")).unwrap();
		let stmt = &p["Statement"][0];
		assert_eq!(stmt["Effect"], "Deny");
		assert_eq!(stmt["Condition"]["Bool"]["aws:SecureTransport"], "false");
		let resources = stmt["Resource"].as_array().unwrap();
		assert!(resources.contains(&serde_json::json!("arn:aws:s3:::bes-canopy-backup-x")));
		assert!(resources.contains(&serde_json::json!("arn:aws:s3:::bes-canopy-backup-x/*")));
	}

	#[test]
	fn lifecycle_builds() {
		// The static rule must construct (catches a bad builder/required-field).
		lifecycle().expect("lifecycle config builds");
	}

	use aws_sdk_s3::operation::create_bucket::{CreateBucketError, CreateBucketOutput};
	use aws_sdk_s3::operation::put_bucket_lifecycle_configuration::PutBucketLifecycleConfigurationOutput;
	use aws_sdk_s3::operation::put_bucket_policy::PutBucketPolicyOutput;
	use aws_sdk_s3::operation::put_bucket_tagging::PutBucketTaggingOutput;
	use aws_sdk_s3::operation::put_bucket_versioning::PutBucketVersioningOutput;
	use aws_sdk_s3::operation::put_object_lock_configuration::PutObjectLockConfigurationOutput;
	use aws_sdk_s3::operation::put_public_access_block::PutPublicAccessBlockOutput;
	use aws_sdk_s3::types::error::BucketAlreadyOwnedByYou;
	use aws_smithy_mocks::{RuleMode, mock, mock_client};

	/// The six post-create config calls, each mocked to succeed.
	macro_rules! config_rules {
		() => {{
			(
				mock!(aws_sdk_s3::Client::put_bucket_versioning)
					.then_output(|| PutBucketVersioningOutput::builder().build()),
				mock!(aws_sdk_s3::Client::put_object_lock_configuration)
					.then_output(|| PutObjectLockConfigurationOutput::builder().build()),
				mock!(aws_sdk_s3::Client::put_public_access_block)
					.then_output(|| PutPublicAccessBlockOutput::builder().build()),
				mock!(aws_sdk_s3::Client::put_bucket_lifecycle_configuration)
					.then_output(|| PutBucketLifecycleConfigurationOutput::builder().build()),
				mock!(aws_sdk_s3::Client::put_bucket_policy)
					.then_output(|| PutBucketPolicyOutput::builder().build()),
				mock!(aws_sdk_s3::Client::put_bucket_tagging)
					.then_output(|| PutBucketTaggingOutput::builder().build()),
			)
		}};
	}

	#[tokio::test]
	async fn apply_recipe_happy_path_runs_the_full_sequence() {
		let create = mock!(aws_sdk_s3::Client::create_bucket)
			.then_output(|| CreateBucketOutput::builder().build());
		let (ver, lock, pab, lc, pol, tag) = config_rules!();
		let s3 = mock_client!(
			aws_sdk_s3,
			RuleMode::MatchAny,
			[&create, &ver, &lock, &pab, &lc, &pol, &tag]
		);
		apply_recipe(
			&s3,
			"bes-canopy-backup-x",
			"ap-southeast-2",
			&[("billing.product".to_string(), "backups".to_string())],
		)
		.await
		.expect("happy path succeeds");
	}

	#[tokio::test]
	async fn apply_recipe_treats_already_owned_bucket_as_success() {
		// A retried init hits an already-owned bucket — that must not fail.
		let create = mock!(aws_sdk_s3::Client::create_bucket).then_error(|| {
			CreateBucketError::BucketAlreadyOwnedByYou(BucketAlreadyOwnedByYou::builder().build())
		});
		let (ver, lock, pab, lc, pol, tag) = config_rules!();
		let s3 = mock_client!(
			aws_sdk_s3,
			RuleMode::MatchAny,
			[&create, &ver, &lock, &pab, &lc, &pol, &tag]
		);
		apply_recipe(&s3, "bes-canopy-backup-x", "ap-southeast-2", &[])
			.await
			.expect("already-owned is idempotent");
	}

	use aws_sdk_s3::operation::get_bucket_tagging::GetBucketTaggingOutput;

	#[tokio::test]
	async fn reconcile_writes_when_billing_tags_missing_and_preserves_others() {
		// Bucket has an unrelated tag but not the billing ones → a PutBucketTagging
		// is required (and the mock only matches if put is actually called).
		let get = mock!(aws_sdk_s3::Client::get_bucket_tagging).then_output(|| {
			GetBucketTaggingOutput::builder()
				.tag_set(Tag::builder().key("team").value("keep").build().unwrap())
				.build()
				.unwrap()
		});
		let put = mock!(aws_sdk_s3::Client::put_bucket_tagging)
			.then_output(|| PutBucketTaggingOutput::builder().build());
		let s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&get, &put]);
		let changed = reconcile_tags(
			&s3,
			"bes-canopy-backup-x",
			&[("billing.product".to_string(), "backups".to_string())],
		)
		.await
		.expect("reconcile ok");
		assert!(changed, "should write when a billing tag is missing");
	}

	#[tokio::test]
	async fn reconcile_is_noop_when_tags_already_match() {
		// No put rule: if reconcile tried to write, the mock would have no match
		// and fail — so this also asserts it does NOT write.
		let get = mock!(aws_sdk_s3::Client::get_bucket_tagging).then_output(|| {
			GetBucketTaggingOutput::builder()
				.tag_set(
					Tag::builder()
						.key("billing.product")
						.value("backups")
						.build()
						.unwrap(),
				)
				.build()
				.unwrap()
		});
		let s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&get]);
		let changed = reconcile_tags(
			&s3,
			"bes-canopy-backup-x",
			&[("billing.product".to_string(), "backups".to_string())],
		)
		.await
		.expect("reconcile ok");
		assert!(!changed, "no write when tags already match");
	}
}
