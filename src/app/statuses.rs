use std::{collections::BTreeSet, net::IpAddr};

use ipnet::IpNet;
use rocket::serde::json::Json;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use serde::Serialize;
use uuid::Uuid;

use crate::{
	db::{
		device_role::DeviceRole,
		devices::{AdminDevice, Device, ServerDevice},
		latest_statuses::LatestStatus,
		server_rank::ServerRank,
		servers::Server,
		statuses::{NewStatus, Status},
		Db,
	},
	error::{AppError, Result},
};

use super::{
	tamanu_headers::{ServerTypeHeader, VersionHeader},
	TamanuHeaders, Version,
};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct LiveVersionsBracket {
	pub min: Version,
	pub max: Version,
}

#[get("/status")]
pub async fn view(mut db: Connection<Db>) -> Result<TamanuHeaders<Template>> {
	let entries = LatestStatus::fetch(&mut db).await;

	let versions = entries
		.iter()
		.filter_map(|status| {
			if let (Some(version), true, ServerRank::Production) = (
				status.latest_success_version.clone(),
				status.is_up,
				status.server_rank,
			) {
				Some(version)
			} else {
				None
			}
		})
		.collect::<BTreeSet<_>>();
	let bracket = LiveVersionsBracket {
		min: versions.first().cloned().unwrap_or_default(),
		max: versions.last().cloned().unwrap_or_default(),
	};
	let releases = versions
		.iter()
		.map(|v| (v.0.major, v.0.minor))
		.collect::<BTreeSet<_>>();
	Ok(TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			entries,
			bracket,
			versions,
			releases,
		},
	)))
}

#[post("/reload")]
pub async fn reload(_device: AdminDevice, mut db: Connection<Db>) -> Result<TamanuHeaders<()>> {
	Status::ping_servers_and_save(&mut db).await;
	Ok(TamanuHeaders::new(()))
}

#[post("/status/<server_id>")]
pub async fn create(
	device: ServerDevice,
	remote_addr: IpAddr,
	server_type: ServerTypeHeader,
	current_version: VersionHeader,
	mut db: Connection<Db>,
	server_id: Uuid,
) -> Result<TamanuHeaders<Json<Status>>> {
	use rocket_db_pools::diesel::prelude::*;
	let Device { role, id, .. } = device.0;

	let is_authorized = role == DeviceRole::Admin || {
		Server::get_by_id(&mut db, server_id).await?.device_id == id
	};

	if !is_authorized {
		return Err(AppError::custom(
			"device is not authorized to create statuses",
		));
	}

	let remote_ip = IpNet::new(remote_addr, 32).unwrap();
	let input = NewStatus {
		server_id,
		latency_ms: None,
		error: None,
		version: Some(current_version.0),
		remote_ip: Some(remote_ip),
		server_type: server_type.into(),
	};

	let status = diesel::insert_into(crate::schema::statuses::table)
		.values(input)
		.returning(Status::as_select())
		.get_result(&mut db)
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(Json(status)))
}
