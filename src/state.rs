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
	pub fn init_db() -> Db {
		Self::init_db_to(&var("DATABASE_URL").expect("DATABASE_URL must be set"))
	}

	pub fn init_db_to(url: &str) -> Db {
		Pool::new(AsyncDieselConnectionManager::<AsyncPgConnection>::new(url))
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
			db: Self::init_db(),
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
