use rocket::mtls::Certificate;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};

use crate::{
	app::TamanuHeaders,
	db::{latest_statuses::LatestStatus, statuses::Status, Db},
};

#[get("/")]
pub async fn view(mut db: Connection<Db>) -> TamanuHeaders<Template> {
	let entries = LatestStatus::fetch(&mut db).await;
	TamanuHeaders::new(Template::render(
		"statuses",
		context! {
			title: "Server statuses",
			entries,
		},
	))
}

#[post("/reload")]
pub async fn reload(_auth: Certificate<'_>, mut db: Connection<Db>) -> TamanuHeaders<()> {
	Status::ping_servers_and_save(&mut db).await;
	TamanuHeaders::new(())
}
