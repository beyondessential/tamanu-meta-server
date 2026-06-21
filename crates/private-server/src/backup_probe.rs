//! Synchronous bucket probe for the setup wizard.
//!
//! Before a config is saved, the wizard assumes the group's **maintenance** role
//! and inspects the target bucket/prefix so the operator gets immediate
//! feedback: empty, an existing kopia repo, other (forgotten) content, or
//! inaccessible. This is **inspect-only** (S3 `HeadObject`/`ListObjectsV2`) —
//! private-server has no kopia binary, so a typed passphrase is verified later
//! at init time (wrong passphrase → `last_init_error`), not here.
//!
//! `Aws` is the real prober; `Fake` is an injectable canned result for tests and
//! the e2e binary (gated by `CANOPY_BACKUP_PROBER_FAKE`), mirroring
//! [`commons_servers::backup_secrets::BackupSecrets`].

use commons_errors::Result;
use serde::Serialize;
use utoipa::ToSchema;

/// Env var that forces the fake prober (no AWS needed). Set by the e2e fixture.
const FAKE_ENV: &str = "CANOPY_BACKUP_PROBER_FAKE";

/// What the probe found at `bucket/prefix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
	/// Nothing there (or only a `.storageconfig`) — safe to create from-birth.
	Empty,
	/// An existing kopia repo (`<prefix>kopia.repository` present) — import only.
	KopiaRepo,
	/// Non-kopia objects present — block; operator must clear it or pick another
	/// prefix/bucket.
	OtherContent,
	/// Couldn't assume the role or list the bucket (creds/role/bucket/region).
	Inaccessible,
}

/// Result of an inspect probe. `already_configured` is filled by the handler
/// (a DB check), not the prober.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProbeResult {
	#[schema(value_type = String)]
	pub state: ProbeState,
	/// Present for `Inaccessible`: the assume/list failure, surfaced-ish.
	pub error: Option<String>,
	/// A few object keys, for the `OtherContent` warning.
	pub object_sample: Vec<String>,
}

impl ProbeResult {
	fn state(state: ProbeState) -> Self {
		Self {
			state,
			error: None,
			object_sample: Vec::new(),
		}
	}
}

/// Bucket prober. `Aws` assumes the role + inspects S3; `Fake` returns a canned
/// result for tests / the e2e binary. `Fake(Some(state))` is a fixed result
/// (Rust unit tests); `Fake(None)` derives the state from the bucket name (e2e,
/// so one binary can exercise every wizard branch — see [`fake_state_for`]).
#[derive(Clone)]
pub enum BucketProber {
	Aws(aws_config::SdkConfig),
	Fake(Option<ProbeState>),
}

impl std::fmt::Debug for BucketProber {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Aws(_) => f.write_str("BucketProber::Aws"),
			Self::Fake(s) => write!(f, "BucketProber::Fake({s:?})"),
		}
	}
}

impl BucketProber {
	/// Build from the ambient AWS config, or the bucket-name-derived fake prober
	/// if `FAKE_ENV` is set (for the e2e binary).
	pub async fn try_default() -> Self {
		if std::env::var_os(FAKE_ENV).is_some() {
			// Debug-only (tests, e2e, local dev). The fake prober can't be
			// constructed in release builds (attribute-`cfg`), so canned probe
			// results can't be swapped in by accident in production — where they'd
			// let onboarding write over a non-empty bucket.
			#[cfg(debug_assertions)]
			{
				tracing::warn!("{FAKE_ENV} set; using fake bucket prober (state from bucket name)");
				return Self::Fake(None);
			}
			#[cfg(not(debug_assertions))]
			tracing::error!(
				"{FAKE_ENV} is set but IGNORED: the fake bucket prober is debug-only and is never used in release builds"
			);
		}
		Self::Aws(aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await)
	}

	/// A fake prober returning a fixed `state` (for Rust unit tests). **Debug-only**:
	/// the constructor doesn't exist in release builds, so a canned prober can't be
	/// swapped in in production.
	#[cfg(debug_assertions)]
	pub fn fake(state: ProbeState) -> Self {
		Self::Fake(Some(state))
	}

	/// Inspect `bucket/prefix` by assuming `maintenance_role_arn`. Never errors
	/// out — an assume/list failure becomes `Inaccessible` with the message.
	pub async fn probe(
		&self,
		bucket: &str,
		prefix: &str,
		region: Option<&str>,
		maintenance_role_arn: &str,
	) -> Result<ProbeResult> {
		match self {
			Self::Fake(Some(state)) => Ok(ProbeResult::state(*state)),
			Self::Fake(None) => Ok(ProbeResult::state(fake_state_for(bucket))),
			Self::Aws(sdk) => {
				Ok(probe_aws(sdk, bucket, prefix, region, maintenance_role_arn).await)
			}
		}
	}
}

/// Bucket-name → fake state, so the e2e suite can drive each wizard branch by
/// naming its bucket (e.g. `…existing…` → an existing repo). Default: `Empty`.
fn fake_state_for(bucket: &str) -> ProbeState {
	if bucket.contains("existing") {
		ProbeState::KopiaRepo
	} else if bucket.contains("other") {
		ProbeState::OtherContent
	} else if bucket.contains("denied") {
		ProbeState::Inaccessible
	} else {
		ProbeState::Empty
	}
}

async fn probe_aws(
	sdk: &aws_config::SdkConfig,
	bucket: &str,
	prefix: &str,
	region: Option<&str>,
	role_arn: &str,
) -> ProbeResult {
	let inaccessible = |e: String| ProbeResult {
		state: ProbeState::Inaccessible,
		error: Some(e),
		object_sample: Vec::new(),
	};

	// Assume the maintenance role (full read) for the inspect.
	let sts = aws_sdk_sts::Client::new(sdk);
	let assumed = match sts
		.assume_role()
		.role_arn(role_arn)
		.role_session_name("canopy-probe")
		.send()
		.await
	{
		Ok(r) => r,
		Err(e) => return inaccessible(format!("AssumeRole failed: {e}")),
	};
	let Some(creds) = assumed.credentials() else {
		return inaccessible("AssumeRole returned no credentials".into());
	};
	let s3_creds = aws_sdk_s3::config::Credentials::new(
		creds.access_key_id(),
		creds.secret_access_key(),
		Some(creds.session_token().to_string()),
		None,
		"canopy-probe",
	);
	let mut b = aws_sdk_s3::config::Builder::from(sdk).credentials_provider(s3_creds);
	if let Some(region) = region {
		b = b.region(aws_sdk_s3::config::Region::new(region.to_string()));
	}
	let s3 = aws_sdk_s3::Client::from_conf(b.build());

	// An existing kopia repo writes its format blob at `<prefix>kopia.repository`.
	let marker = format!("{prefix}kopia.repository");
	if s3
		.head_object()
		.bucket(bucket)
		.key(&marker)
		.send()
		.await
		.is_ok()
	{
		return ProbeResult::state(ProbeState::KopiaRepo);
	}

	// List a few objects to distinguish empty (or `.storageconfig`-only) from
	// other content.
	let listed = match s3
		.list_objects_v2()
		.bucket(bucket)
		.prefix(prefix)
		.max_keys(20)
		.send()
		.await
	{
		Ok(o) => o,
		Err(e) => return inaccessible(format!("ListObjectsV2 failed: {e}")),
	};
	let storageconfig = format!("{prefix}.storageconfig");
	let other: Vec<String> = listed
		.contents()
		.iter()
		.filter_map(|o| o.key().map(str::to_string))
		.filter(|k| *k != storageconfig)
		.collect();

	if other.is_empty() {
		ProbeResult::state(ProbeState::Empty)
	} else {
		ProbeResult {
			state: ProbeState::OtherContent,
			error: None,
			object_sample: other.into_iter().take(5).collect(),
		}
	}
}
