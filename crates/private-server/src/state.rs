use std::sync::{Arc, Mutex};

use axum::extract::FromRef;
use bestool_postgres::pool::PgPool;
use commons_errors::Result;
use commons_servers::backup_jobs::SharedBackupConfig;
use commons_servers::recovery_vault::Recipients;
use commons_servers::tailnet_directory::{TailnetDirectory, TailnetDirectoryConfig};
use database::Db;
use public_server::state::BackupSecrets;

use crate::backup_probe::BucketProber;

/// A pending recovery vault verification challenge: the random nonce Canopy issued
/// (encrypted to the recipients) and when, so [`crate::fns::backups::recovery_verify`]
/// can match the operator's decrypted answer and reject stale ones.
#[derive(Clone, Debug)]
pub struct RecoveryChallenge {
	pub nonce: String,
	pub issued_at: jiff::Timestamp,
}

/// At most one in-flight recovery-vault challenge at a time (operator-driven).
pub type RecoveryChallengeStore = Arc<Mutex<Option<RecoveryChallenge>>>;

#[derive(Clone, Debug, FromRef)]
pub struct AppState {
	pub db: Db,
	pub ro_pool: Option<PgPool>,
	pub tailnet_directory: Option<TailnetDirectory>,
	/// Secret store for the per-group repo-password Secrets (onboarding creates
	/// them here). `None` in non-cluster runs ⇒ onboarding returns 502. Reuses
	/// the public-server's `BackupSecrets` (kube or in-memory).
	pub kube: Option<BackupSecrets>,
	/// Bucket prober for the setup wizard (assume role + inspect S3). `Aws` in
	/// prod; a `Fake` canned result in tests / the e2e binary.
	pub prober: BucketProber,
	/// recovery vault recipient public keys (`CANOPY_RECOVERY_VAULT_KEYS`), for the
	/// verification ceremony. `None` ⇒ the ceremony endpoints 502 (the backups
	/// pod is what hard-requires them, not this admin server).
	pub recovery_recipients: Option<Recipients>,
	/// The single in-flight recovery verification challenge, if any.
	pub recovery_challenge: RecoveryChallengeStore,
	/// Shared-account backup settings (`CANOPY_SHARED_BACKUP_*`). `None` ⇒ the
	/// shared-account onboarding endpoint 502s; BYO (`external`) onboarding is
	/// unaffected.
	pub shared_backups: Option<SharedBackupConfig>,
}

/// Read the recovery recipients from the environment, logging (not failing) a malformed
/// list — the admin server should still come up; the ceremony endpoints surface
/// the misconfiguration.
fn recovery_recipients_from_env() -> Option<Recipients> {
	match Recipients::from_env() {
		Ok(r) => r,
		Err(e) => {
			tracing::warn!(
				"ignoring malformed {}: {e}",
				commons_servers::recovery_vault::RECIPIENTS_ENV
			);
			None
		}
	}
}

impl AppState {
	pub async fn init() -> Result<Self> {
		let ro_pool = if let Ok(url) = std::env::var("RO_DATABASE_URL") {
			(bestool_postgres::pool::create_pool(&url, "canopy-playground").await).ok()
		} else {
			None
		};

		let tailnet_directory = match TailnetDirectoryConfig::from_env()? {
			Some(config) => Some(TailnetDirectory::new(config).await?),
			None => None,
		};

		let kube = BackupSecrets::try_default().await;
		let prober = BucketProber::try_default().await;

		Ok(Self {
			db: database::init(),
			ro_pool,
			tailnet_directory,
			kube,
			prober,
			recovery_recipients: recovery_recipients_from_env(),
			recovery_challenge: Arc::new(Mutex::new(None)),
			shared_backups: SharedBackupConfig::from_env(),
		})
	}

	/// Test/e2e-only AppState builder. Debug-only because it constructs the
	/// debug-only in-memory Secret store and fake prober (`BackupSecrets::memory`
	/// / `BucketProber::fake`), which don't exist in release builds.
	#[cfg(debug_assertions)]
	pub async fn from_db_url(url: &str) -> Result<Self> {
		Ok(Self {
			db: database::init_to(url),
			ro_pool: None,
			tailnet_directory: None,
			// In-memory secret store so onboarding (Secret creation) is exercised
			// in tests without a cluster.
			kube: Some(BackupSecrets::memory()),
			// Bucket-name-derived fake prober (matches the e2e fixture): a test
			// drives each probe state by naming the bucket — `…existing…` → kopia
			// repo, `…other…` → other content, `…denied…` → inaccessible, else empty.
			prober: BucketProber::Fake(None),
			// Read from env so the e2e fixture can exercise the recovery ceremony.
			recovery_recipients: recovery_recipients_from_env(),
			recovery_challenge: Arc::new(Mutex::new(None)),
			// A canned shared-account config so the shared-onboarding endpoint is
			// exercisable in tests/e2e without real AWS. Placeholder account id.
			shared_backups: Some(SharedBackupConfig {
				region: "ap-southeast-2".to_string(),
				device_role_arn: "arn:aws:iam::123456789012:role/canopy-shared-device".to_string(),
				maintenance_role_arn: "arn:aws:iam::123456789012:role/canopy-shared-maint"
					.to_string(),
				provisioner_role_arn: "arn:aws:iam::123456789012:role/canopy-shared-provisioner"
					.to_string(),
			}),
		})
	}
}
