use std::collections::BTreeSet;

use rocket::mtls::Certificate;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};

use crate::{
	app::TamanuHeaders,
	db::{latest_statuses::LatestStatus, server_rank::ServerRank, statuses::Status, Db},
};

use super::versions::LiveVersionsBracket;

#[get("/")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Template> {
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
		min: versions.first().cloned().unwrap(),
		max: versions.last().cloned().unwrap(),
	};
	let releases = versions
		.iter()
		.map(|v| (v.0.major, v.0.minor))
		.collect::<BTreeSet<_>>();
	TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			entries,
			bracket,
			versions,
			releases,
		},
	))
}

#[post("/reload")]
pub async fn reload(_auth: Certificate<'_>, mut db: Connection<Db>) -> TamanuHeaders<()> {
	Status::ping_servers_and_save(&mut db).await;
	TamanuHeaders::new(())
}
