use commons_errors::Result;
use leptos::server;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[cfg(feature = "ssr")]
mod ssr {
	use super::*;
	use axum::extract::State;
	use bestool_postgres::pool;
	use bestool_postgres::stringify::postgres_to_json_value;
	use bestool_postgres::text_cast::{CellRef, TextCaster};
	use commons_errors::Result;
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
		let State(ro_pool): State<Option<pool::PgPool>> = extract_with_state(&state).await?;

		match ro_pool {
			Some(pool) => {
				let start_time = Instant::now();

				// Get connection from pool
				let mut client = pool.get().await.map_err(|e| {
					commons_errors::AppError::custom(format!("Failed to get connection: {}", e))
				})?;

				// Start a transaction with timeout and read-only settings
				let transaction = client
					.build_transaction()
					.read_only(true)
					.start()
					.await
					.map_err(|e| {
						commons_errors::AppError::custom(format!(
							"Failed to start transaction: {}",
							e
						))
					})?;

				// Set session as read-only
				transaction
					.execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY", &[])
					.await
					.map_err(|e| {
						commons_errors::AppError::custom(format!("Failed to set read-only: {}", e))
					})?;

				// Execute the query with timeout
				let rows = tokio::time::timeout(
					std::time::Duration::from_secs(60),
					transaction.query(&query.query, &[]),
				)
				.await
				.map_err(|_| {
					commons_errors::AppError::custom("Query execution timed out after 60 seconds")
				})?
				.map_err(|e| {
					commons_errors::AppError::custom(format!("Query execution failed: {}", e))
				})?;

				// Rollback the transaction (cancel it)
				transaction.rollback().await.map_err(|e| {
					commons_errors::AppError::custom(format!(
						"Failed to rollback transaction: {}",
						e
					))
				})?;

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

				// Convert rows to JSON values using bestool-postgres
				// First pass: convert all values and collect null cells for text casting
				let mut null_cells = Vec::new();
				let mut all_values = Vec::new();

				for (row_idx, row) in rows.iter().enumerate() {
					let mut row_values = Vec::with_capacity(columns.len());
					for col_idx in 0..columns.len() {
						// Use bestool-postgres to convert PostgreSQL values to JSON
						let value = postgres_to_json_value(&row, col_idx);

						// Store the value
						row_values.push(value);

						// If value is Null, mark it for text casting
						if let serde_json::Value::Null = &row_values[col_idx] {
							null_cells.push(CellRef { row_idx, col_idx });
						}
					}
					all_values.push(row_values);
				}

				// If we have null cells, try to cast them to text using TextCaster
				if !null_cells.is_empty() {
					let text_caster = TextCaster::new(pool.clone());
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
								all_values[cell_ref.row_idx][cell_ref.col_idx] =
									serde_json::Value::Null;
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
			None => Err(commons_errors::AppError::custom(
				"SQL functionality is disabled (RO_DATABASE_URL not set)",
			)),
		}
	}
}
