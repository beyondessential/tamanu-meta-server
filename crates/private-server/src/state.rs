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
	/// Kube-backed reader for the per-group repo-password Secrets. `None` in
	/// tests / non-cluster runs ⇒ the admin escrow-reveal endpoint returns 502.
	/// Reuses the public-server's narrow Secret-read wrapper.
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

	pub async fn from_db_url(url: &str) -> Result<Self> {
		Ok(Self {
			db: database::init_to(url),
			ro_pool: None,
			tailnet_directory: None,
			// In-memory secret store so onboarding (Secret creation) + escrow
			// reveal are exercised in tests without a cluster.
			kube: Some(BackupSecrets::memory()),
		})
	}
}
