use axum::extract::FromRef;
use bestool_postgres::pool::PgPool;
use commons_errors::Result;
use commons_servers::tailnet_directory::{TailnetDirectory, TailnetDirectoryConfig};
use database::Db;

#[derive(Clone, Debug, FromRef)]
pub struct AppState {
	pub db: Db,
	pub ro_pool: Option<PgPool>,
	pub tailnet_directory: Option<TailnetDirectory>,
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

		Ok(Self {
			db: database::init(),
			ro_pool,
			tailnet_directory,
		})
	}

	pub async fn from_db_url(url: &str) -> Result<Self> {
		Ok(Self {
			db: database::init_to(url),
			ro_pool: None,
			tailnet_directory: None,
		})
	}
}
