use axum::{
	Json,
	extract::{Path, State},
	routing::{Router, post},
};

use diesel::SelectableHelper as _;
use diesel_async::RunQueryDsl as _;
use uuid::Uuid;

use crate::{
	db::{
		device_role::DeviceRole,
		devices::Device,
		servers::Server,
		statuses::{NewStatus, Status},
	},
	error::{AppError, Result},
	servers::{device_auth::ServerDevice, headers::VersionHeader},
	state::{AppState, Db},
};

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
	let Device { role, id, .. } = device.0;

	let is_authorized = role == DeviceRole::Admin || {
		Server::get_by_id(&mut db, server_id).await?.device_id == Some(id)
	};

	if !is_authorized {
		return Err(AppError::custom(
			"device is not authorized to create statuses",
		));
	}

	let input = NewStatus {
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
	};

	let status = diesel::insert_into(crate::schema::statuses::table)
		.values(input)
		.returning(Status::as_select())
		.get_result(&mut db)
		.await?;

	Ok(Json(status))
}
