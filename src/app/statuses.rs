use rocket::mtls::Certificate;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};

use crate::{
	app::TamanuHeaders,
	db::{latest_statuses::LatestStatus, statuses::Status, Db},
};

use super::versions::LiveVersionsBracket;

#[get("/")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Template> {
	let entries = LatestStatus::fetch(&mut db).await;

	let mut versions = entries
		.iter()
		.filter_map(|status| {
			if let (Some(version), true) = (status.latest_success_version.clone(), status.is_up) {
				Some(version)
			} else {
				None
			}
		})
		.collect::<Vec<_>>();
	versions.sort();
	let bracket = LiveVersionsBracket {
		min: versions.first().cloned().unwrap(),
		max: versions.last().cloned().unwrap(),
	};
	TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			entries,
			bracket,
			versions,
		},
	))
}

#[post("/reload")]
pub async fn reload(_auth: Certificate<'_>, mut db: Connection<Db>) -> TamanuHeaders<()> {
	Status::ping_servers_and_save(&mut db).await;
	TamanuHeaders::new(())
}
