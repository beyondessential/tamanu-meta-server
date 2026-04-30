use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::Result;
use commons_servers::tailscale_auth::TailscaleAdmin;
use serde::Deserialize;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/list", post(list))
		.route("/add", post(add))
		.route("/delete", post(delete))
}

pub async fn list(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
) -> Result<Json<Vec<String>>> {
	let mut conn = state.db.get().await?;
	let admins = database::admins::Admin::list(&mut conn)
		.await?
		.into_iter()
		.map(|a| a.email)
		.collect();
	Ok(Json(admins))
}

#[derive(Deserialize)]
pub struct AddArgs {
	pub email: String,
}

pub async fn add(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<AddArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	database::admins::Admin::add(&mut conn, &args.email).await?;
	Ok(Json(()))
}

#[derive(Deserialize)]
pub struct DeleteArgs {
	pub email: String,
}

pub async fn delete(
	State(state): State<AppState>,
	TailscaleAdmin(_): TailscaleAdmin,
	Json(args): Json<DeleteArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	database::admins::Admin::delete(&mut conn, &args.email).await?;
	Ok(Json(()))
}
