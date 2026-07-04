use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::fns::Page;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BestoolSnippetInfo {
	pub id: Uuid,
	pub name: String,
	pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BestoolSnippetDetail {
	pub id: Uuid,
	pub name: String,
	pub description: Option<String>,
	pub sql: String,
	pub editor: String,
}

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list_snippets))
		.routes(routes!(save_snippet))
		.routes(routes!(get_snippet))
		.routes(routes!(get_latest_snippet_id))
		.routes(routes!(delete_snippet))
}

#[derive(Deserialize, ToSchema)]
pub struct BestoolListArgs {
	pub offset: u64,
	pub limit: Option<u64>,
}

#[utoipa::path(
	post,
	path = "/list_snippets",
	tag = "bestool",
	request_body = BestoolListArgs,
	responses(
		(status = 200, body = Page<BestoolSnippetInfo>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn list_snippets(
	State(state): State<AppState>,
	Json(args): Json<BestoolListArgs>,
) -> Result<Json<Page<BestoolSnippetInfo>>> {
	let mut conn = state.db.get().await?;
	let total = database::BestoolSnippet::count_current(&mut conn).await? as u64;
	let snippets = database::BestoolSnippet::list_current(
		&mut conn,
		args.offset as i64,
		args.limit.unwrap_or(50) as i64,
	)
	.await?;
	let items = snippets
		.into_iter()
		.map(|s| BestoolSnippetInfo {
			id: s.id,
			name: s.name,
			description: s.description,
		})
		.collect();
	Ok(Json(Page { items, total }))
}

#[derive(Deserialize, ToSchema)]
pub struct SaveArgs {
	/// When set, the saved snippet supersedes the snippet with this id (i.e.
	/// it's an edit). When absent, a fresh snippet is created.
	pub supersedes: Option<Uuid>,
	pub name: String,
	pub description: Option<String>,
	pub sql: String,
}

#[utoipa::path(
	post,
	path = "/save_snippet",
	tag = "bestool",
	security(("tailscale-user" = [])),
	request_body = SaveArgs,
	responses(
		(status = 200, body = BestoolSnippetDetail),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn save_snippet(
	State(state): State<AppState>,
	user: std::result::Result<TailscaleUser, AppError>,
	Json(args): Json<SaveArgs>,
) -> Result<Json<BestoolSnippetDetail>> {
	let user = user.unwrap_or_default();
	let mut conn = state.db.get().await?;
	let snippet = database::BestoolSnippet::create(
		&mut conn,
		user.login,
		args.name,
		args.description,
		args.sql,
		args.supersedes,
	)
	.await?;
	Ok(Json(BestoolSnippetDetail {
		id: snippet.id,
		name: snippet.name,
		description: snippet.description,
		sql: snippet.sql,
		editor: snippet.editor,
	}))
}

#[derive(Deserialize, ToSchema)]
pub struct GetArgs {
	pub id: Uuid,
}

#[utoipa::path(
	post,
	path = "/get_snippet",
	tag = "bestool",
	request_body = GetArgs,
	responses(
		(status = 200, body = BestoolSnippetDetail),
		(status = 404, body = ProblemDetailsSchema),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn get_snippet(
	State(state): State<AppState>,
	Json(args): Json<GetArgs>,
) -> Result<Json<BestoolSnippetDetail>> {
	let mut conn = state.db.get().await?;
	let snippet = database::BestoolSnippet::get_by_id(&mut conn, args.id)
		.await?
		.ok_or_else(|| AppError::custom("Snippet not found"))?;
	Ok(Json(BestoolSnippetDetail {
		id: snippet.id,
		name: snippet.name,
		description: snippet.description,
		sql: snippet.sql,
		editor: snippet.editor,
	}))
}

#[utoipa::path(
	post,
	path = "/get_latest_snippet_id",
	tag = "bestool",
	request_body = GetArgs,
	responses(
		(status = 200, description = "The newest id in the supersedes chain rooted at the given id.", body = Uuid, content_type = "application/json"),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn get_latest_snippet_id(
	State(state): State<AppState>,
	Json(args): Json<GetArgs>,
) -> Result<Json<Uuid>> {
	let mut conn = state.db.get().await?;
	let id = database::BestoolSnippet::get_latest_id(&mut conn, args.id).await?;
	Ok(Json(id))
}

#[utoipa::path(
	post,
	path = "/delete_snippet",
	tag = "bestool",
	request_body = GetArgs,
	responses(
		(status = 200, description = "Snippet soft-deleted."),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn delete_snippet(
	State(state): State<AppState>,
	Json(args): Json<GetArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	let _ = database::BestoolSnippet::delete(&mut conn, args.id).await?;
	Ok(Json(()))
}
