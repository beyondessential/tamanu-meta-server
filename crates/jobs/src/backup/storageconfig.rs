//! Ensure the bucket's `.storageconfig` (kopia Intelligent-Tiering policy) exists.
//!
//! Pulumi normally writes `.storageconfig` at bucket creation; Canopy writes it
//! as a **fallback** on repo init when it's absent, and **never overwrites** an
//! existing one. Best-effort: a missing `.storageconfig` only affects S3 tiering,
//! not backup correctness, so init logs + continues on failure.
//!
//! Schema mirrors ops (`pulumi/.../backup/kopia.ts`): data blobs (the `p`
//! prefix) → Intelligent-Tiering; everything else → Standard so indexes stay in
//! the frequent-access tier.

use anyhow::{Context, Result};

const STORAGECONFIG_JSON: &str = r#"{
  "blobOptions": [
    { "prefix": "p", "storageClass": "INTELLIGENT_TIERING" },
    { "storageClass": "STANDARD" }
  ]
}
"#;

/// Create `<prefix>.storageconfig` if absent (assuming the maintenance role).
/// Never overwrites an existing object.
pub async fn ensure(
	maintenance_role_arn: &str,
	bucket: &str,
	prefix: &str,
	region: Option<&str>,
) -> Result<()> {
	let sdk = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
	let sts = aws_sdk_sts::Client::new(&sdk);
	let assumed = sts
		.assume_role()
		.role_arn(maintenance_role_arn)
		.role_session_name("canopy-storageconfig")
		.send()
		.await
		.context("assume maintenance role")?;
	let c = assumed
		.credentials()
		.context("AssumeRole returned no credentials")?;
	let creds = aws_sdk_s3::config::Credentials::new(
		c.access_key_id(),
		c.secret_access_key(),
		Some(c.session_token().to_string()),
		None,
		"canopy-storageconfig",
	);
	let mut b = aws_sdk_s3::config::Builder::from(&sdk).credentials_provider(creds);
	if let Some(region) = region {
		b = b.region(aws_sdk_s3::config::Region::new(region.to_string()));
	}
	let s3 = aws_sdk_s3::Client::from_conf(b.build());

	let key = format!("{prefix}.storageconfig");
	// Never overwrite an existing config.
	if s3
		.head_object()
		.bucket(bucket)
		.key(&key)
		.send()
		.await
		.is_ok()
	{
		return Ok(());
	}
	s3.put_object()
		.bucket(bucket)
		.key(&key)
		.body(aws_sdk_s3::primitives::ByteStream::from_static(
			STORAGECONFIG_JSON.as_bytes(),
		))
		.content_type("application/json")
		.send()
		.await
		.context("put .storageconfig")?;
	Ok(())
}
