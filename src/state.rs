use std::env::var;

use axum::extract::FromRef;
use diesel_async::{
	AsyncPgConnection,
	pooled_connection::{AsyncDieselConnectionManager, mobc::Pool},
};

use crate::error::AppError;

pub type Db = Pool<AsyncPgConnection>;

#[derive(Clone, Debug)]
pub struct AppState {
	pub db: Db,
}

impl AppState {
	pub fn init() -> Result<Self, AppError> {
		let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(var("DATABASE_URL")?);
		let db = Pool::new(config);

		Ok(Self { db })
	}
}

impl FromRef<AppState> for Db {
	fn from_ref(state: &AppState) -> Self {
		state.db.clone()
	}
}
