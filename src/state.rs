use std::env::var;

use diesel_async::{
	AsyncPgConnection,
	pooled_connection::{AsyncDieselConnectionManager, mobc::Pool},
};

use crate::error::AppError;

#[derive(Clone, Debug)]
pub struct AppState {
	pub db: Pool<AsyncPgConnection>,
}

impl AppState {
	pub fn init() -> Result<Self, AppError> {
		let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(var("DATABASE_URL")?);
		let db = Pool::new(config);

		Ok(Self { db })
	}
}
