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
				let client = pool.get().await.map_err(|e| {
					commons_errors::AppError::custom(format!("Failed to get connection: {}", e))
				})?;

				// Execute the query
				let rows = client.query(&query.query, &[]).await.map_err(|e| {
					commons_errors::AppError::custom(format!("Query execution failed: {}", e))
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
				let mut result_rows = Vec::with_capacity(rows.len());
				for row in &rows {
					let mut row_values = Vec::with_capacity(columns.len());
					for i in 0..columns.len() {
						// Use bestool-postgres to convert PostgreSQL values to JSON
						let value = postgres_to_json_value(&row, i);
						row_values.push(value);
					}
					result_rows.push(row_values);
				}

				Ok(SqlResult {
					columns,
					rows: result_rows,
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
