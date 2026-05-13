use axum::{
	Json,
	extract::{Path, State},
};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::{device_auth::ServerDevice, headers::VersionHeader};
use commons_types::device::DeviceRole;
use database::{
	Db,
	devices::Device,
	servers::Server,
	statuses::{NewStatus, Status},
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(create))
}

#[utoipa::path(
	post,
	path = "/{server_id}",
	tag = "statuses",
	security(("server-device" = [])),
	params(
		("server_id" = Uuid, Path),
	),
	request_body(
		content = serde_json::Value,
		description = "Optional free-form `extra` payload. Empty body or JSON `null` are both treated as `{}`.",
	),
	responses(
		(status = 200, body = Status),
		(status = 400, body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
async fn create(
	Path(server_id): Path<Uuid>,
	State(db): State<Db>,
	device: ServerDevice,
	current_version: VersionHeader,
	body: Option<Json<serde_json::Value>>,
) -> Result<Json<Status>> {
	let mut db = db.get().await?;
	let Device { role, id, .. } = device.0.0;

	let is_authorized = role == DeviceRole::Admin || {
		Server::get_by_id(&mut db, server_id).await?.device_id == Some(id)
	};

	if !is_authorized {
		return Err(AppError::custom(
			"device is not authorized to create statuses",
		));
	}

	let raw = body.map(|j| j.0).unwrap_or(serde_json::Value::Null);
	let (healthy, health, extra) = split_health_from_extra(raw)?;

	let status = NewStatus {
		server_id,
		device_id: Some(id),
		version: Some(current_version.0),
		extra,
		healthy,
		health,
	}
	.save(&mut db)
	.await?;

	Ok(Json(status))
}

/// Pulls the reserved `healthy` and `health` keys out of the incoming
/// status body and returns them alongside the rest of the payload
/// (`extra`). Validates types per the contract:
///
/// - missing or `null` body → `healthy = true`, `health = []`, `extra = {}`
/// - `healthy` absent ⇒ `true` (legacy compat — non-negotiable, this is
///   what stops every legacy server from false-positiving unhealthy on
///   the day we deploy)
/// - `healthy` present must be a bool
/// - `health` if present must be an array of objects, each with at
///   least `check: non-empty string` and `healthy: bool`. Other fields
///   on each entry are passed through verbatim.
fn split_health_from_extra(
	raw: serde_json::Value,
) -> Result<(bool, serde_json::Value, serde_json::Value)> {
	let mut obj = match raw {
		serde_json::Value::Null => serde_json::Map::new(),
		serde_json::Value::Object(m) => m,
		_ => {
			return Err(AppError::BadRequest(
				"status body must be a JSON object (or null/empty)".into(),
			));
		}
	};

	let healthy = match obj.remove("healthy") {
		None => true,
		Some(serde_json::Value::Bool(b)) => b,
		Some(_) => return Err(AppError::BadRequest("`healthy` must be a boolean".into())),
	};

	let health_value = obj
		.remove("health")
		.unwrap_or(serde_json::Value::Array(Default::default()));
	let health_arr = match health_value {
		serde_json::Value::Array(a) => a,
		_ => return Err(AppError::BadRequest("`health` must be an array".into())),
	};
	for (idx, entry) in health_arr.iter().enumerate() {
		let Some(entry_obj) = entry.as_object() else {
			return Err(AppError::BadRequest(format!(
				"`health[{idx}]` must be an object",
			)));
		};
		match entry_obj.get("check") {
			Some(serde_json::Value::String(s)) if !s.is_empty() => {}
			Some(_) | None => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}].check` must be a non-empty string",
				)));
			}
		}
		match entry_obj.get("healthy") {
			Some(serde_json::Value::Bool(_)) => {}
			Some(_) | None => {
				return Err(AppError::BadRequest(format!(
					"`health[{idx}].healthy` must be a boolean",
				)));
			}
		}
	}

	Ok((healthy, serde_json::Value::Array(health_arr), serde_json::Value::Object(obj)))
}
