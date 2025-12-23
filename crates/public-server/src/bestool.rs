use axum::{
	Json,
	extract::State,
	routing::Router,
	routing::get,
};
use commons_errors::Result;
use database::Db;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetResponse {
	pub description: Option<String>,
	pub sql: String,
}

pub fn routes() -> Router<AppState> {
	Router::new().route("/snippets", get(list_snippets))
}

#[axum::debug_handler]
async fn list_snippets(State(db): State<Db>) -> Result<Json<BTreeMap<String, SnippetResponse>>> {
	let mut conn = db.get().await?;

	let snippets = database::BestoolSnippet::list_current(&mut conn, 0, i64::MAX).await?;

	let response = snippets
		.into_iter()
		.map(|s| {
			(
				s.name,
				SnippetResponse {
					description: s.description,
					sql: s.sql,
				},
			)
		})
		.collect();

	Ok(Json(response))
}
