use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(add))
		.routes(routes!(delete))
}

#[utoipa::path(
	post,
	path = "/list",
	operation_id = "admin_list",
	tag = "admins",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, description = "Admin emails.", body = Vec<String>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn list(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
) -> Result<Json<Vec<String>>> {
	let mut conn = state.db.get().await?;
	let admins = database::admins::Admin::list(&mut conn)
		.await?
		.into_iter()
		.map(|a| a.email)
		.collect();
	Ok(Json(admins))
}

#[derive(Deserialize, ToSchema)]
pub struct AddArgs {
	pub email: String,
}

#[utoipa::path(
	post,
	path = "/add",
	operation_id = "admin_add",
	tag = "admins",
	security(("tailscale-admin" = [])),
	request_body = AddArgs,
	responses(
		(status = 200, description = "Admin added (idempotent)."),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn add(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<AddArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	database::admins::Admin::add(&mut conn, &args.email).await?;
	Ok(Json(()))
}

#[derive(Deserialize, ToSchema)]
pub struct DeleteArgs {
	pub email: String,
}

#[utoipa::path(
	post,
	path = "/delete",
	operation_id = "admin_delete",
	tag = "admins",
	security(("tailscale-admin" = [])),
	request_body = DeleteArgs,
	responses(
		(status = 200, description = "Admin removed (idempotent)."),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn delete(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<DeleteArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	database::admins::Admin::delete(&mut conn, &args.email).await?;
	Ok(Json(()))
}
