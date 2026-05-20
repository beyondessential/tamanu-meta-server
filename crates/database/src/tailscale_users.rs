//! Cached display metadata for Tailscale users (name + profile picture).
//!
//! Handlers that record "this human did X" (issue resolve, incident ack/resolve)
//! upsert here from the request's Tailscale headers so the rest of the API
//! can render avatars without having to round-trip through the Tailscale
//! API.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::tailscale_users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TailscaleUser {
	pub login: String,
	pub name: String,
	pub profile_pic: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

impl TailscaleUser {
	/// Upsert the user's display info. Called from handlers that record
	/// human actions so the latest name/pic from Tailscale headers is
	/// available to the UI.
	pub async fn upsert(
		db: &mut AsyncPgConnection,
		login: &str,
		name: &str,
		profile_pic: Option<&str>,
	) -> Result<()> {
		use crate::schema::tailscale_users::dsl;

		diesel::insert_into(dsl::tailscale_users)
			.values((
				dsl::login.eq(login),
				dsl::name.eq(name),
				dsl::profile_pic.eq(profile_pic),
			))
			.on_conflict(dsl::login)
			.do_update()
			.set((dsl::name.eq(name), dsl::profile_pic.eq(profile_pic)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Bulk lookup keyed by login. Missing logins are simply omitted from
	/// the map. Used by issue/incident endpoints that embed acker/resolver
	/// display info into their responses.
	pub async fn by_logins(
		db: &mut AsyncPgConnection,
		logins: &[&str],
	) -> Result<HashMap<String, Self>> {
		use crate::schema::tailscale_users::dsl;

		if logins.is_empty() {
			return Ok(HashMap::new());
		}
		let rows: Vec<Self> = dsl::tailscale_users
			.select(Self::as_select())
			.filter(dsl::login.eq_any(logins))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows.into_iter().map(|u| (u.login.clone(), u)).collect())
	}
}
