use std::collections::BTreeSet;

use rocket_db_pools::{diesel::prelude::*, Connection};
use rocket_dyn_templates::{context, Template};
use rocket::serde::json::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::{
	db::{
		devices::{AdminDevice, ServerDevice},
		latest_statuses::LatestStatus,
		server_rank::ServerRank,
		statuses::{Status, NewStatus},
		Db,
	},
	error::{AppError, Result},
};

use super::{TamanuHeaders, Version};

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
		min: versions.iter().next().cloned().unwrap_or_default(),
		max: versions.iter().next_back().cloned().unwrap_or_default(),
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

#[post("/status/<server_id>/<current_version>")]
pub async fn create(
	_device: ServerDevice,
	mut db: Connection<Db>,
	server_id: Uuid,
	current_version: Version,
) -> Result<TamanuHeaders<Json<Status>>> {
	let input = NewStatus {
		server_id,
		latency_ms: None,
		error: None,
		version: Some(current_version),
	};

	let status = diesel::insert_into(crate::schema::statuses::table)
		.values(input)
		.returning(Status::as_select())
		.get_result(&mut db)
		.await
		.map_err(|err| AppError::Database(err.to_string()))?;

	Ok(TamanuHeaders::new(Json(status)))
}
