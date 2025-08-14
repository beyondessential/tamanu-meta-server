use std::net::IpAddr;

use ipnet::IpNet;
use rocket::serde::json::Json;
use rocket_db_pools::Connection;
use serde::Serialize;
use uuid::Uuid;

use crate::{
	db::{
		Db,
		device_role::DeviceRole,
		devices::Device,
		servers::Server,
		statuses::{NewStatus, Status},
	},
	error::{AppError, Result},
	servers::{
		device_auth::ServerDevice,
		headers::{TamanuHeaders, VersionHeader},
		version::Version,
	},
};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct LiveVersionsBracket {
	pub min: Version,
	pub max: Version,
}

#[post("/status/<server_id>", data = "<extra>")]
pub async fn create(
	device: ServerDevice,
	remote_addr: IpAddr,
	current_version: VersionHeader,
	mut db: Connection<Db>,
	server_id: Uuid,
	extra: Option<Json<serde_json::Value>>,
) -> Result<TamanuHeaders<Json<Status>>> {
	use rocket_db_pools::diesel::prelude::*;
	let Device { role, id, .. } = device.0;

	let is_authorized = role == DeviceRole::Admin || {
		Server::get_by_id(&mut db, server_id).await?.device_id == Some(id)
	};

	if !is_authorized {
		return Err(AppError::custom(
			"device is not authorized to create statuses",
		));
	}

	let remote_ip = IpNet::new(remote_addr, 32).unwrap();
	let input = NewStatus {
		server_id,
		device_id: Some(id),
		version: Some(current_version.0),
		remote_ip: Some(remote_ip),
		extra: extra.map_or_else(
			|| serde_json::Value::Object(Default::default()),
			|j| match j.0 {
				serde_json::Value::Null => serde_json::Value::Object(Default::default()),
				v => v,
			},
		),
		..Default::default()
	};

	let status = diesel::insert_into(crate::schema::statuses::table)
		.values(input)
		.returning(Status::as_select())
		.get_result(&mut db)
		.await?;

	Ok(TamanuHeaders::new(Json(status)))
}
