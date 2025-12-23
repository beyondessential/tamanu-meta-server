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
pub async fn create_snippet(
	name: String,
	description: Option<String>,
	sql: String,
) -> Result<()> {
	ssr::create_snippet(name, description, sql).await
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

		database::BestoolSnippet::count_active(&mut conn)
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

		let snippets = database::BestoolSnippet::list_active(
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
		let user: commons_servers::tailscale_auth::TailscaleUser = extract_with_state(&state).await.unwrap_or_default();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		database::BestoolSnippet::create(
			&mut conn,
			user.login,
			name,
			description,
			sql,
			None,
		)
		.await?;

		Ok(())
	}
}
