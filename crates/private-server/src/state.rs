use axum::extract::FromRef;
use commons_errors::Result;
use database::Db;

#[derive(Clone, Debug)]
pub struct AppState {
	pub db: Db,
}

impl AppState {
	pub fn init() -> Result<Self> {
		Ok(Self {
			db: database::init(),
		})
	}
}

impl FromRef<AppState> for Db {
	fn from_ref(state: &AppState) -> Self {
		state.db.clone()
	}
}
