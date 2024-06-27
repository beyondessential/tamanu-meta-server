use rocket_db_pools::{diesel::PgPool, Database};

pub mod pg_duration;
pub mod server_rank;
pub mod servers;
pub mod url_field;

#[derive(Clone, Database)]
#[database("postgres")]
pub struct Db(PgPool);
