use std::collections::BTreeSet;

use rocket_db_pools::Connection;
use rocket_dyn_templates::{Template, context};
use serde::Serialize;

use crate::{
	db::{Db, latest_statuses::LatestStatus, server_rank::ServerRank, statuses::Status},
	error::Result,
	servers::{headers::TamanuHeaders, version::Version},
};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct LiveVersionsBracket {
	pub min: Version,
	pub max: Version,
}

#[get("/status")]
pub async fn view(mut db: Connection<Db>) -> Result<TamanuHeaders<Template>> {
	let entries = LatestStatus::fetch(&mut db).await?;

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
pub async fn reload(mut db: Connection<Db>) -> Result<TamanuHeaders<()>> {
	Status::ping_servers_and_save(&mut db).await?;
	Ok(TamanuHeaders::new(()))
}
