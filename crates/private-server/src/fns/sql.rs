use commons_errors::Result;
use jiff::Timestamp;
use leptos::server;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlQuery {
	pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlResult {
	pub columns: Vec<String>,
	pub rows: Vec<Vec<Value>>,
	pub row_count: usize,
	pub execution_time_ms: u64,
}

#[server]
pub async fn is_sql_available() -> Result<bool> {
	ssr::is_sql_available().await
}

#[server]
pub async fn execute_query(query: SqlQuery) -> Result<SqlResult> {
	ssr::execute_query(query).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlHistoryEntry {
	pub id: Uuid,
	pub query: String,
	pub tailscale_user: String,
	pub created_at: Timestamp,
}

#[server]
pub async fn get_last_user_query() -> Result<Option<String>> {
	ssr::get_last_user_query().await
}

#[server]
pub async fn get_query_history_count() -> Result<u64> {
	ssr::get_query_history_count().await
}

#[server]
pub async fn get_query_history(offset: u64, limit: Option<u64>) -> Result<Vec<SqlHistoryEntry>> {
	ssr::get_query_history(offset, limit).await
}

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use axum::extract::State;
	use bestool_postgres::error::format_db_error;
	use bestool_postgres::pool;
	use bestool_postgres::stringify::postgres_to_json_value;
	use bestool_postgres::text_cast::{CellRef, TextCaster};
	use commons_errors::Result;
	use commons_servers::tailscale_auth::TailscaleUser;
	use database::Db;
	use database::sql_playground_history::SqlPlaygroundHistory;
	use leptos::prelude::expect_context;
	use leptos_axum::extract_with_state;
	use std::time::Instant;

	use crate::state::AppState;

	pub async fn is_sql_available() -> Result<bool> {
		let state = expect_context::<AppState>();
		let State(ro_pool): State<Option<pool::PgPool>> = extract_with_state(&state).await?;
		Ok(ro_pool.is_some())
	}

	pub async fn execute_query(query: SqlQuery) -> Result<SqlResult> {
		let state = expect_context::<AppState>();
		let State(Some(ro_pool)): State<Option<pool::PgPool>> = extract_with_state(&state).await?
		else {
			return Err(commons_errors::AppError::custom(
				"SQL functionality is disabled (RO_DATABASE_URL not set)",
			));
		};

		let start_time = Instant::now();

		let state = expect_context::<AppState>();
		let user: TailscaleUser = extract_with_state(&state).await.unwrap_or_default();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		SqlPlaygroundHistory::create(&mut conn, query.query.clone(), user.login.clone())
			.await
			.map_err(|e| {
				commons_errors::AppError::custom(format!("Failed to record query history: {}", e))
			})?;

		let mut client = ro_pool.get().await.map_err(|e| {
			commons_errors::AppError::custom(format!("Failed to get connection: {}", e))
		})?;

		let transaction = client
			.build_transaction()
			.read_only(true)
			.start()
			.await
			.map_err(|e| commons_errors::AppError::custom(format_db_error(&e, None)))?;

		transaction
			.execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY", &[])
			.await
			.map_err(|e| {
				commons_errors::AppError::custom(format_db_error(
					&e,
					Some("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY"),
				))
			})?;

		let rows = tokio::time::timeout(
			std::time::Duration::from_secs(60),
			transaction.query(&query.query, &[]),
		)
		.await
		.map_err(|_| {
			commons_errors::AppError::custom("Query execution timed out after 60 seconds")
		})?
		.map_err(|e| commons_errors::AppError::custom(format_db_error(&e, Some(&query.query))))?;

		// explicitly rollback the transaction out of precaution
		transaction
			.rollback()
			.await
			.map_err(|e| commons_errors::AppError::custom(format_db_error(&e, None)))?;

		let execution_time = start_time.elapsed();

		if rows.is_empty() {
			return Ok(SqlResult {
				columns: Vec::new(),
				rows: Vec::new(),
				row_count: 0,
				execution_time_ms: execution_time.as_millis() as u64,
			});
		}

		// Get column names from the first row
		let first_row = &rows[0];
		let columns: Vec<String> = first_row
			.columns()
			.iter()
			.map(|col| col.name().to_string())
			.collect();

		// First pass: convert all values and collect null cells for text casting
		let mut null_cells = Vec::new();
		let mut all_values = Vec::new();

		for (row_idx, row) in rows.iter().enumerate() {
			let mut row_values = Vec::with_capacity(columns.len());
			for col_idx in 0..columns.len() {
				let value = postgres_to_json_value(row, col_idx);

				row_values.push(value);

				// If value is Null, mark it for text casting
				// (it could be a genuine null, or a failure to get a string)
				if let serde_json::Value::Null = &row_values[col_idx] {
					null_cells.push(CellRef { row_idx, col_idx });
				}
			}
			all_values.push(row_values);
		}

		// If we have null cells, try to cast them to text using TextCaster
		if !null_cells.is_empty() {
			let text_caster = TextCaster::new(ro_pool.clone());
			let text_results = text_caster.cast_batch(&rows, &null_cells).await;

			// Update the null values with text representations
			for (cell_ref, text_result) in null_cells.iter().zip(text_results) {
				match text_result {
					Ok(text) => {
						all_values[cell_ref.row_idx][cell_ref.col_idx] =
							serde_json::Value::String(text);
					}
					Err(_) => {
						// Keep as Null if text casting fails
						all_values[cell_ref.row_idx][cell_ref.col_idx] = serde_json::Value::Null;
					}
				}
			}
		}

		Ok(SqlResult {
			columns,
			rows: all_values,
			row_count: rows.len(),
			execution_time_ms: execution_time.as_millis() as u64,
		})
	}

	pub async fn get_last_user_query() -> Result<Option<String>> {
		let state = expect_context::<AppState>();
		let user: TailscaleUser = extract_with_state(&state).await.unwrap_or_default();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		Ok(
			SqlPlaygroundHistory::get_last_by_user(&mut conn, &user.login)
				.await?
				.map(|entry| entry.query),
		)
	}

	pub async fn get_query_history_count() -> Result<u64> {
		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;
		Ok(SqlPlaygroundHistory::count_all(&mut conn)
			.await?
			.try_into()
			.unwrap_or(0))
	}

	pub async fn get_query_history(
		offset: u64,
		limit: Option<u64>,
	) -> Result<Vec<SqlHistoryEntry>> {
		let limit = limit.unwrap_or(10) as i64;
		let offset = offset as i64;

		let state = expect_context::<AppState>();
		let State(db): State<Db> = extract_with_state(&state).await?;
		let mut conn = db.get().await?;

		Ok(
			SqlPlaygroundHistory::get_paginated(&mut conn, offset, limit)
				.await?
				.into_iter()
				.map(|entry| SqlHistoryEntry {
					id: entry.id,
					query: entry.query,
					tailscale_user: entry.tailscale_user,
					created_at: entry.created_at,
				})
				.collect(),
		)
	}
}
