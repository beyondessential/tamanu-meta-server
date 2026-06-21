use axum::extract::FromRef;
use bestool_postgres::pool::PgPool;
use commons_errors::Result;
use commons_servers::tailnet_directory::{TailnetDirectory, TailnetDirectoryConfig};
use database::Db;
use public_server::state::BackupSecrets;

#[derive(Clone, Debug, FromRef)]
pub struct AppState {
	pub db: Db,
	pub ro_pool: Option<PgPool>,
	pub tailnet_directory: Option<TailnetDirectory>,
	/// Secret store for the per-group repo-password Secrets (onboarding creates
	/// them here). `None` in non-cluster runs ⇒ onboarding returns 502. Reuses
	/// the public-server's `BackupSecrets` (kube or in-memory).
	pub kube: Option<BackupSecrets>,
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

		Ok(Self {
			db: database::init(),
			ro_pool,
			tailnet_directory,
			kube,
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
		})
	}
}
