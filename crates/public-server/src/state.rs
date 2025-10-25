#[cfg(feature = "ui")]
use std::sync::Arc;

use axum::extract::FromRef;
use commons_errors::Result;
use database::Db;
#[cfg(feature = "ui")]
use tera::Tera;

#[derive(Clone, Debug)]
pub struct AppState {
	pub db: Db,
	#[cfg(feature = "ui")]
	pub tera: Arc<Tera>,
	#[cfg(feature = "ui")]
	pub server_versions_secret: Option<String>,
}

impl AppState {
	#[cfg(feature = "ui")]
	pub fn init_tera() -> Result<Arc<Tera>> {
		let mut tera = Tera::default();

		macro_rules! embed_template {
			($name:expr) => {
				tera.add_raw_template(
					$name,
					include_str!(concat!("../templates/", $name, ".html.tera")),
				)
				.unwrap();
			};
		}

		embed_template!("artifacts");
		embed_template!("mobile");
		embed_template!("password");
		embed_template!("server_versions");
		embed_template!("versions");

		Ok(Arc::new(tera))
	}

	pub fn init() -> Result<Self> {
		Self::from_db(database::init())
	}

	pub fn from_db(db: Db) -> Result<Self> {
		Ok(Self {
			db,
			#[cfg(feature = "ui")]
			tera: Self::init_tera()?,
			#[cfg(feature = "ui")]
			server_versions_secret: std::env::var("SERVER_VERSIONS_SECRET").ok(),
		})
	}
}

impl FromRef<AppState> for Db {
	fn from_ref(state: &AppState) -> Self {
		state.db.clone()
	}
}

#[cfg(feature = "ui")]
impl FromRef<AppState> for Arc<Tera> {
	fn from_ref(state: &AppState) -> Self {
		state.tera.clone()
	}
}
