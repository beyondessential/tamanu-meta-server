use rocket::serde::{json::Json, Serialize};
use rocket_db_pools::Connection;

use crate::{
	app::{TamanuHeaders, Version},
	db::{latest_statuses::LatestStatus, server_rank::ServerRank},
	Db,
};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct LiveVersionsBracket {
	pub min: Version,
	pub max: Version,
}

#[get("/versions")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Json<LiveVersionsBracket>> {
	let statuses = LatestStatus::only_up(&mut db).await;
	let mut versions = statuses
		.iter()
		.filter_map(|status| {
			if let (Some(version), ServerRank::Production) =
				(status.latest_success_version.clone(), status.server_rank)
			{
				Some(version)
			} else {
				None
			}
		})
		.collect::<Vec<_>>();
	versions.sort();
	let min = versions.first().cloned().expect("no versions returned");
	let max = versions.last().cloned().expect("no versions returned");
	TamanuHeaders::new(LiveVersionsBracket { min, max }.into())
}
