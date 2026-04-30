use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestoolSnippetInfo {
	pub id: Uuid,
	pub name: String,
	pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestoolSnippetDetail {
	pub id: Uuid,
	pub name: String,
	pub description: Option<String>,
	pub sql: String,
	pub editor: String,
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/count_snippets", post(count_snippets))
		.route("/list_snippets", post(list_snippets))
		.route("/create_snippet", post(create_snippet))
		.route("/get_snippet", post(get_snippet))
		.route("/get_latest_snippet_id", post(get_latest_snippet_id))
		.route("/update_snippet", post(update_snippet))
		.route("/delete_snippet", post(delete_snippet))
}

pub async fn count_snippets(State(state): State<AppState>) -> Result<Json<u64>> {
	let mut conn = state.db.get().await?;
	let count = database::BestoolSnippet::count_current(&mut conn).await? as u64;
	Ok(Json(count))
}

#[derive(Deserialize)]
pub struct ListArgs {
	pub offset: u64,
	pub limit: Option<u64>,
}

pub async fn list_snippets(
	State(state): State<AppState>,
	Json(args): Json<ListArgs>,
) -> Result<Json<Vec<BestoolSnippetInfo>>> {
	let mut conn = state.db.get().await?;
	let snippets = database::BestoolSnippet::list_current(
		&mut conn,
		args.offset as i64,
		args.limit.unwrap_or(50) as i64,
	)
	.await?;
	Ok(Json(
		snippets
			.into_iter()
			.map(|s| BestoolSnippetInfo {
				id: s.id,
				name: s.name,
				description: s.description,
			})
			.collect(),
	))
}

#[derive(Deserialize)]
pub struct CreateArgs {
	pub name: String,
	pub description: Option<String>,
	pub sql: String,
}

pub async fn create_snippet(
	State(state): State<AppState>,
	user: std::result::Result<TailscaleUser, AppError>,
	Json(args): Json<CreateArgs>,
) -> Result<Json<()>> {
	let user = user.unwrap_or_default();
	let mut conn = state.db.get().await?;
	database::BestoolSnippet::create(
		&mut conn,
		user.login,
		args.name,
		args.description,
		args.sql,
		None,
	)
	.await?;
	Ok(Json(()))
}

#[derive(Deserialize)]
pub struct GetArgs {
	pub id: Uuid,
}

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

pub async fn get_latest_snippet_id(
	State(state): State<AppState>,
	Json(args): Json<GetArgs>,
) -> Result<Json<Uuid>> {
	let mut conn = state.db.get().await?;
	let id = database::BestoolSnippet::get_latest_id(&mut conn, args.id).await?;
	Ok(Json(id))
}

#[derive(Deserialize)]
pub struct UpdateArgs {
	pub id: Uuid,
	pub name: String,
	pub description: Option<String>,
	pub sql: String,
}

pub async fn update_snippet(
	State(state): State<AppState>,
	user: std::result::Result<TailscaleUser, AppError>,
	Json(args): Json<UpdateArgs>,
) -> Result<Json<BestoolSnippetDetail>> {
	let user = user.unwrap_or_default();
	let mut conn = state.db.get().await?;
	let new_snippet = database::BestoolSnippet::create(
		&mut conn,
		user.login,
		args.name,
		args.description,
		args.sql,
		Some(args.id),
	)
	.await?;
	Ok(Json(BestoolSnippetDetail {
		id: new_snippet.id,
		name: new_snippet.name,
		description: new_snippet.description,
		sql: new_snippet.sql,
		editor: new_snippet.editor,
	}))
}

pub async fn delete_snippet(
	State(state): State<AppState>,
	Json(args): Json<GetArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	let _ = database::BestoolSnippet::delete(&mut conn, args.id).await?;
	Ok(Json(()))
}
