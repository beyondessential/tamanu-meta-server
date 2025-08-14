use axum::routing::{Router, any};
use rocket_db_pools::Connection;

use crate::{db::Db, error::Result};

#[get("/readyz")]
pub async fn ready(_db: Connection<Db>) -> Result<()> {
	Ok(())
}

#[get("/livez")]
pub async fn live(_db: Connection<Db>) -> Result<()> {
	Ok(())
}

pub fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
	Router::new()
		.route("/livez", any(async || {}))
		.route("/healthz", any(async || {}))
}
