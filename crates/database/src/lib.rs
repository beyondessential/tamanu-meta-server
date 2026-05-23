use diesel_async::{
	AsyncPgConnection,
	pooled_connection::{AsyncDieselConnectionManager, mobc::Pool},
};

pub mod admins;
pub mod artifacts;
pub mod bestool_snippets;
pub mod chrome_releases;
pub mod devices;
pub mod issues;
pub mod notes;
pub mod pg_duration;
pub mod schema;
pub mod server_groups;
pub mod servers;
pub mod silenced_refs;
pub mod slack_outbox;
pub mod sql_playground_history;
pub mod statuses;
pub mod tailscale_users;
pub mod url_field;
pub mod version_known_issues;
pub mod versions;
pub mod views;

pub use bestool_snippets::{BestoolSnippet, NewBestoolSnippet};
pub use devices::{Device, DeviceConnection, DeviceKey, DeviceWithInfo};

pub type Db = Pool<AsyncPgConnection>;

// Re-export for use in other crates
pub use diesel_async;

pub fn init() -> Db {
	init_to(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
}

pub fn init_to(url: &str) -> Db {
	Pool::new(AsyncDieselConnectionManager::<AsyncPgConnection>::new(url))
}
