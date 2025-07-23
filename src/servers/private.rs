use rocket::{Build, Rocket, fs::FileServer};
use rocket_db_pools::Database as _;
use rocket_dyn_templates::Template;

use crate::db::Db;

pub use super::{headers::TamanuHeaders, health};

pub mod statuses;

#[catch(404)]
fn not_found() -> TamanuHeaders<()> {
	TamanuHeaders::new(())
}

pub fn rocket(prefix: String) -> Rocket<Build> {
	rocket::build()
		.attach(Template::fairing())
		.attach(Db::init())
		.register(format!("{prefix}/"), catchers![not_found])
		.mount(
			format!("{prefix}/"),
			routes![
				health::ready,
				health::live,
				statuses::view,
				statuses::reload,
			],
		)
		.mount(format!("{prefix}/static"), FileServer::from("static"))
}
