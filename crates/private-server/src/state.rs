use axum::extract::FromRef;
use bestool_postgres::pool::PgPool;
use commons_errors::Result;
use database::Db;
use leptos::config::{LeptosOptions, get_configuration};

#[derive(Clone, Debug, FromRef)]
pub struct AppState {
	pub db: Db,
	pub ro_pool: Option<PgPool>,
	pub leptos_options: LeptosOptions,
}

impl AppState {
	pub async fn init() -> Result<Self> {
		let conf = get_configuration(None).unwrap();

		let ro_pool = if let Ok(url) = std::env::var("RO_DATABASE_URL") {
			(bestool_postgres::pool::create_pool(&url, "tamanu-meta-playground").await).ok()
		} else {
			None
		};

		Ok(Self {
			db: database::init(),
			ro_pool,
			leptos_options: conf.leptos_options,
		})
	}

	pub async fn from_db_url(url: &str) -> Result<Self> {
		let conf = get_configuration(None).unwrap();

		Ok(Self {
			db: database::init_to(url),
			ro_pool: None,
			leptos_options: conf.leptos_options,
		})
	}
}
