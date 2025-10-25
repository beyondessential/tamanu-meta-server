use axum::{
	Json,
	extract::{Path, State},
	routing::{Router, post},
};
use commons_errors::{AppError, Result};
use commons_servers::{device_auth::ServerDevice, headers::VersionHeader};
use commons_types::device::DeviceRole;
use database::{
	Db,
	devices::Device,
	servers::Server,
	statuses::{NewStatus, Status},
};
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
	Router::new().route("/{server_id}", post(create))
}

async fn create(
	Path(server_id): Path<Uuid>,
	State(db): State<Db>,
	device: ServerDevice,
	current_version: VersionHeader,
	extra: Option<Json<serde_json::Value>>,
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

	let status = NewStatus {
		server_id,
		device_id: Some(id),
		version: Some(current_version.0),
		extra: extra.map_or_else(
			|| serde_json::Value::Object(Default::default()),
			|j| match j.0 {
				serde_json::Value::Null => serde_json::Value::Object(Default::default()),
				v => v,
			},
		),
	}
	.save(&mut db)
	.await?;

	Ok(Json(status))
}
