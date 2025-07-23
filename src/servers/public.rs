use rocket::{Build, Rocket, fs::FileServer};
use rocket_db_pools::Database as _;
use rocket_dyn_templates::Template;

use crate::{db::Db, servers::version::Version};

pub use super::headers::TamanuHeaders;

pub mod artifacts;
pub mod password;
pub mod servers;
pub mod statuses;
pub mod timesync;
pub mod versions;

#[catch(404)]
fn not_found() -> TamanuHeaders<()> {
	TamanuHeaders::new(())
}

pub fn rocket() -> Rocket<Build> {
	rocket::build()
		.attach(Template::fairing())
		.attach(Db::init())
		.register("/", catchers![not_found])
		.mount(
			"/",
			routes![
				servers::list,
				servers::create,
				servers::edit,
				servers::delete,
				statuses::create,
				timesync::endpoint,
				versions::list,
				versions::view,
				versions::create,
				versions::delete,
				versions::update_for,
				versions::get_artifacts,
				versions::view_artifacts,
				versions::view_mobile_install,
				artifacts::create,
				password::view,
			],
		)
		.mount("/static", FileServer::from("static"))
}
