use rocket_db_pools::{diesel::PgPool, Database};

pub mod artifacts;
pub mod device_role;
pub mod devices;
pub mod latest_statuses;
pub mod pg_duration;
pub mod server_kind;
pub mod server_rank;
pub mod servers;
pub mod statuses;
pub mod url_field;
pub mod versions;

#[derive(Clone, Database)]
#[database("postgres")]
pub struct Db(PgPool);
