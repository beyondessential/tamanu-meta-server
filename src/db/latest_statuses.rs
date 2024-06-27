use chrono::{DateTime, Utc};
use rocket::serde::Serialize;
use rocket_db_pools::diesel::{prelude::*, AsyncPgConnection};
use uuid::Uuid;

use crate::{
	app::Version,
	db::{pg_duration::PgHumanDuration, server_rank::ServerRank, url_field::UrlField},
};

#[derive(Debug, Clone, Serialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::views::latest_statuses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct LatestStatus {
	pub server_id: Uuid,
	pub server_created_at: DateTime<Utc>,
	pub server_updated_at: DateTime<Utc>,
	pub server_name: String,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub server_rank: ServerRank,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub server_host: UrlField,

	pub is_up: bool,
	pub latest_latency: Option<i32>,

	pub latest_success_id: Option<Uuid>,
	pub latest_success_ts: Option<DateTime<Utc>>,
	pub latest_success_ago: Option<PgHumanDuration>,
	pub latest_success_version: Option<Version>,

	pub latest_error_id: Option<Uuid>,
	pub latest_error_ts: Option<DateTime<Utc>>,
	pub latest_error_ago: Option<PgHumanDuration>,
	pub latest_error_message: Option<String>,
}

impl LatestStatus {
	pub async fn fetch(db: &mut AsyncPgConnection) -> Vec<Self> {
		use crate::views::latest_statuses::dsl::*;

		latest_statuses
			.select(Self::as_select())
			.load(db)
			.await
			.expect("Error loading statuses")
	}
}
