use rocket::{Build, Rocket, fs::FileServer};
use rocket_db_pools::Database as _;
use rocket_dyn_templates::Template;

use crate::db::Db;

pub use super::headers::TamanuHeaders;

pub mod statuses;

#[catch(404)]
fn not_found() -> TamanuHeaders<()> {
	TamanuHeaders::new(())
}

pub fn rocket() -> Rocket<Build> {
	rocket::build()
		.attach(Template::fairing())
		.attach(Db::init())
		.register("/", catchers![not_found])
		.mount("/", routes![statuses::view, statuses::reload])
		.mount("/static", FileServer::from("static"))
}
