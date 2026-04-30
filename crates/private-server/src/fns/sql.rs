use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use bestool_postgres::error::format_db_error;
use bestool_postgres::stringify::postgres_to_json_value;
use bestool_postgres::text_cast::{CellRef, TextCaster};
use commons_errors::{AppError, Result};
use commons_servers::tailscale_auth::TailscaleUser;
use database::sql_playground_history::SqlPlaygroundHistory;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::fns::Page;
use crate::state::AppState;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlHistoryEntry {
	pub id: Uuid,
	pub query: String,
	pub tailscale_user: String,
	pub created_at: Timestamp,
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/is_sql_available", post(is_sql_available))
		.route("/execute_query", post(execute_query))
		.route("/get_last_user_query", post(get_last_user_query))
		.route("/get_query_history", post(get_query_history))
}

pub async fn is_sql_available(State(state): State<AppState>) -> Json<bool> {
	Json(state.ro_pool.is_some())
}

#[derive(Deserialize)]
pub struct ExecuteArgs {
	pub query: SqlQuery,
}

pub async fn execute_query(
	State(state): State<AppState>,
	user: std::result::Result<TailscaleUser, AppError>,
	Json(args): Json<ExecuteArgs>,
) -> Result<Json<SqlResult>> {
	let Some(ro_pool) = state.ro_pool.clone() else {
		return Err(AppError::custom(
			"SQL functionality is disabled (RO_DATABASE_URL not set)",
		));
	};

	let query = args.query;
	let user = user.unwrap_or_default();
	let start_time = Instant::now();

	let mut conn = state.db.get().await?;
	SqlPlaygroundHistory::create(&mut conn, query.query.clone(), user.login.clone())
		.await
		.map_err(|e| AppError::custom(format!("Failed to record query history: {}", e)))?;

	let mut client = ro_pool
		.get()
		.await
		.map_err(|e| AppError::custom(format!("Failed to get connection: {}", e)))?;

	let transaction = client
		.build_transaction()
		.read_only(true)
		.start()
		.await
		.map_err(|e| AppError::custom(format_db_error(&e, None)))?;

	transaction
		.execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY", &[])
		.await
		.map_err(|e| {
			AppError::custom(format_db_error(
				&e,
				Some("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY"),
			))
		})?;

	let rows = tokio::time::timeout(
		Duration::from_secs(60),
		transaction.query(&query.query, &[]),
	)
	.await
	.map_err(|_| AppError::custom("Query execution timed out after 60 seconds"))?
	.map_err(|e| AppError::custom(format_db_error(&e, Some(&query.query))))?;

	transaction
		.rollback()
		.await
		.map_err(|e| AppError::custom(format_db_error(&e, None)))?;

	let execution_time = start_time.elapsed();

	if rows.is_empty() {
		return Ok(Json(SqlResult {
			columns: Vec::new(),
			rows: Vec::new(),
			row_count: 0,
			execution_time_ms: execution_time.as_millis() as u64,
		}));
	}

	let first_row = &rows[0];
	let columns: Vec<String> = first_row
		.columns()
		.iter()
		.map(|col| col.name().to_string())
		.collect();

	let mut null_cells = Vec::new();
	let mut all_values = Vec::new();
	for (row_idx, row) in rows.iter().enumerate() {
		let mut row_values = Vec::with_capacity(columns.len());
		for col_idx in 0..columns.len() {
			let value = postgres_to_json_value(row, col_idx);
			row_values.push(value);
			if let Value::Null = &row_values[col_idx] {
				null_cells.push(CellRef { row_idx, col_idx });
			}
		}
		all_values.push(row_values);
	}

	if !null_cells.is_empty() {
		let text_caster = TextCaster::new(ro_pool.clone());
		let text_results = text_caster.cast_batch(&rows, &null_cells).await;
		for (cell_ref, text_result) in null_cells.iter().zip(text_results) {
			match text_result {
				Ok(text) => {
					all_values[cell_ref.row_idx][cell_ref.col_idx] = Value::String(text);
				}
				Err(_) => {
					all_values[cell_ref.row_idx][cell_ref.col_idx] = Value::Null;
				}
			}
		}
	}

	Ok(Json(SqlResult {
		columns,
		rows: all_values,
		row_count: rows.len(),
		execution_time_ms: execution_time.as_millis() as u64,
	}))
}

pub async fn get_last_user_query(
	State(state): State<AppState>,
	user: std::result::Result<TailscaleUser, AppError>,
) -> Result<Json<Option<String>>> {
	let user = user.unwrap_or_default();
	let mut conn = state.db.get().await?;
	let last = SqlPlaygroundHistory::get_last_by_user(&mut conn, &user.login)
		.await?
		.map(|entry| entry.query);
	Ok(Json(last))
}

#[derive(Deserialize)]
pub struct HistoryArgs {
	pub offset: u64,
	pub limit: Option<u64>,
}

pub async fn get_query_history(
	State(state): State<AppState>,
	Json(args): Json<HistoryArgs>,
) -> Result<Json<Page<SqlHistoryEntry>>> {
	let limit = args.limit.unwrap_or(10) as i64;
	let offset = args.offset as i64;
	let mut conn = state.db.get().await?;
	let total = SqlPlaygroundHistory::count_all(&mut conn)
		.await?
		.try_into()
		.unwrap_or(0);
	let items = SqlPlaygroundHistory::get_paginated(&mut conn, offset, limit)
		.await?
		.into_iter()
		.map(|entry| SqlHistoryEntry {
			id: entry.id,
			query: entry.query,
			tailscale_user: entry.tailscale_user,
			created_at: entry.created_at,
		})
		.collect();
	Ok(Json(Page { items, total }))
}
