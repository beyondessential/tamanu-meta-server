use rocket::{Build, Rocket};
use rocket_db_pools::Database as _;
use rocket_dyn_templates::Template;

use crate::db::Db;

pub use self::{tamanu_headers::TamanuHeaders, version::Version};

pub mod server_type;
pub mod servers;
pub mod statuses;
pub mod tamanu_headers;
pub mod version;
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
				statuses::view,
				statuses::reload,
				versions::view,
				versions::update_for,
			],
		)
}
