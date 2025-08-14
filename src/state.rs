use std::{env::var, sync::Arc};

use axum::extract::FromRef;
use diesel_async::{
	AsyncPgConnection,
	pooled_connection::{AsyncDieselConnectionManager, mobc::Pool},
};
use tera::Tera;

use crate::error::Result;

pub type Db = Pool<AsyncPgConnection>;

#[derive(Clone, Debug)]
pub struct AppState {
	pub db: Db,
	pub tera: Arc<Tera>,
}

impl AppState {
	pub fn init_db() -> Result<Db> {
		let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(var("DATABASE_URL")?);
		Ok(Pool::new(config))
	}

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
		embed_template!("statuses");
		embed_template!("versions");

		Ok(Arc::new(tera))
	}

	pub fn init() -> Result<Self> {
		Ok(Self {
			db: Self::init_db()?,
			tera: Self::init_tera()?,
		})
	}
}

impl FromRef<AppState> for Db {
	fn from_ref(state: &AppState) -> Self {
		state.db.clone()
	}
}

impl FromRef<AppState> for Arc<Tera> {
	fn from_ref(state: &AppState) -> Self {
		state.tera.clone()
	}
}
