use diesel_async::{
	AsyncPgConnection,
	pooled_connection::{AsyncDieselConnectionManager, mobc::Pool},
};

pub mod admins;
pub mod artifacts;
pub mod device_role;
pub mod devices;
pub mod migrator;
pub mod pg_duration;
pub mod schema;
pub mod server_kind;
pub mod server_rank;
pub mod servers;
pub mod statuses;
pub mod url_field;
pub mod versions;
pub mod views;

// Re-export commonly used types
pub use device_role::DeviceRole;
pub use devices::{Device, DeviceConnection, DeviceKey, DeviceWithInfo};

pub type Db = Pool<AsyncPgConnection>;

pub fn init() -> Db {
	init_to(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
}

pub fn init_to(url: &str) -> Db {
	Pool::new(AsyncDieselConnectionManager::<AsyncPgConnection>::new(url))
}
