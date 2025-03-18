use rocket::serde::{Deserialize, Serialize};
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, QueryableByName)]
#[diesel(table_name = crate::schema::versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Version {
	pub id: Uuid,
	pub major: i32,
	pub minor: i32,
	pub patch: i32,
	pub published: bool,
	pub changelog: String,
}

impl Version {
	pub async fn get_all(db: &mut AsyncPgConnection) -> Vec<Self> {
		use crate::schema::versions::*;

		table
			.select(Version::as_select())
			.order_by(major.desc())
			.then_order_by(minor.desc())
			.then_order_by(patch.desc())
			.load(db)
			.await
			.expect("Error loading versions")
	}
}
