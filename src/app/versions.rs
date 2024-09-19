use rocket::serde::{json::Json, Serialize};
use rocket_db_pools::Connection;

use crate::{
	app::{TamanuHeaders, Version},
	db::latest_statuses::LatestStatus,
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
	let versions = statuses
		.iter()
		.filter_map(|status| status.latest_success_version.clone())
		.collect::<Vec<_>>();
	let min = versions
		.iter()
		.min()
		.cloned()
		.expect("no versions returned");
	let max = versions
		.iter()
		.max()
		.cloned()
		.expect("no versions returned");
	TamanuHeaders::new(LiveVersionsBracket { min, max }.into())
}
