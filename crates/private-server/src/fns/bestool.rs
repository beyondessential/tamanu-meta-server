use std::sync::Arc;

use commons_errors::Result;
use leptos::server;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[server]
pub async fn count_snippets() -> Result<u64> {
	ssr::count_snippets().await
}

#[server]
pub async fn list_snippets(
	offset: u64,
	limit: Option<u64>,
) -> Result<Vec<Arc<BestoolSnippetInfo>>> {
	ssr::list_snippets(offset, limit).await
}

#[server]
pub async fn create_snippet(name: String, description: Option<String>, sql: String) -> Result<()> {
	ssr::create_snippet(name, description, sql).await
}

#[server]
pub async fn get_snippet(id: Uuid) -> Result<BestoolSnippetDetail> {
	ssr::get_snippet(id).await
}

#[server]
pub async fn get_latest_snippet_id(id: Uuid) -> Result<Uuid> {
	ssr::get_latest_snippet_id(id).await
}

#[server]
pub async fn update_snippet(
	id: Uuid,
	name: String,
	description: Option<String>,
	sql: String,
) -> Result<BestoolSnippetDetail> {
	ssr::update_snippet(id, name, description, sql).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use axum::extract::State;
	use database::Db;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;

	pub async fn count_snippets() -> Result<u64> {
		let state = expect_context::<crate::state::AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		database::BestoolSnippet::count_current(&mut conn)
			.await
			.map(|c| c as u64)
	}

	pub async fn list_snippets(
		offset: u64,
		limit: Option<u64>,
	) -> Result<Vec<Arc<BestoolSnippetInfo>>> {
		let state = expect_context::<crate::state::AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let snippets = database::BestoolSnippet::list_current(
			&mut conn,
			offset as i64,
			limit.unwrap_or(50) as i64,
		)
		.await?;

		Ok(snippets
			.into_iter()
			.map(|s| {
				Arc::new(BestoolSnippetInfo {
					id: s.id,
					name: s.name,
					description: s.description,
				})
			})
			.collect())
	}

	pub async fn create_snippet(
		name: String,
		description: Option<String>,
		sql: String,
	) -> Result<()> {
		let state = expect_context::<crate::state::AppState>();
		let user: commons_servers::tailscale_auth::TailscaleUser =
			extract_with_state(&state).await.unwrap_or_default();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		database::BestoolSnippet::create(&mut conn, user.login, name, description, sql, None)
			.await?;

		Ok(())
	}

	pub async fn get_snippet(id: Uuid) -> Result<super::BestoolSnippetDetail> {
		let state = expect_context::<crate::state::AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		let snippet = database::BestoolSnippet::get_by_id(&mut conn, id)
			.await?
			.ok_or_else(|| commons_errors::AppError::custom("Snippet not found"))?;

		Ok(super::BestoolSnippetDetail {
			id: snippet.id,
			name: snippet.name,
			description: snippet.description,
			sql: snippet.sql,
			editor: snippet.editor,
		})
	}

	pub async fn get_latest_snippet_id(id: Uuid) -> Result<Uuid> {
		let state = expect_context::<crate::state::AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		database::BestoolSnippet::get_latest_id(&mut conn, id).await
	}

	pub async fn update_snippet(
		id: Uuid,
		name: String,
		description: Option<String>,
		sql: String,
	) -> Result<super::BestoolSnippetDetail> {
		let state = expect_context::<crate::state::AppState>();
		let user: commons_servers::tailscale_auth::TailscaleUser =
			extract_with_state(&state).await.unwrap_or_default();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		// Create a new version that supersedes the current one
		let new_snippet = database::BestoolSnippet::create(
			&mut conn,
			user.login,
			name,
			description,
			sql,
			Some(id),
		)
		.await?;

		Ok(super::BestoolSnippetDetail {
			id: new_snippet.id,
			name: new_snippet.name,
			description: new_snippet.description,
			sql: new_snippet.sql,
			editor: new_snippet.editor,
		})
	}
}
