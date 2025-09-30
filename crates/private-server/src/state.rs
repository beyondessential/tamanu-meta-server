use axum::extract::FromRef;
use commons_errors::Result;
use database::Db;
use leptos::config::{LeptosOptions, get_configuration};

#[derive(Clone, Debug, FromRef)]
pub struct AppState {
	pub db: Db,
	pub leptos_options: LeptosOptions,
}

impl AppState {
	pub fn init() -> Result<Self> {
		let conf = get_configuration(None).unwrap();

		Ok(Self {
			db: database::init(),
			leptos_options: conf.leptos_options,
		})
	}

	pub fn from_db_url(url: &str) -> Result<Self> {
		let conf = get_configuration(None).unwrap();

		Ok(Self {
			db: database::init_to(url),
			leptos_options: conf.leptos_options,
		})
	}
}
