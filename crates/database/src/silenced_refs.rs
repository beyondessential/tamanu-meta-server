//! Operator-managed silence list for issue refs.
//!
//! A silenced `(source, ref)` tuple at server or group scope tells the
//! incident workflow to ignore the matching issues — they still record
//! (so the issue and event rows exist), but
//! [`crate::issues::re_evaluate_incident_membership`] treats them as a
//! "should leave" reason, the same way it treats snoozed or unmonitored.
//!
//! Two sibling tables (`server_silenced_refs`, `server_group_silenced_refs`)
//! keep referential integrity tight without nullable FKs. A given issue is
//! silenced if either applies (server-scope wins for the server itself,
//! group-scope catches the whole group).

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::issues::{reevaluate_open_issues_for_group_ref, reevaluate_open_issues_for_server_ref};

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_silenced_refs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerSilencedRef {
	pub server_id: Uuid,
	pub source: String,
	#[diesel(column_name = ref_)]
	#[serde(rename = "ref")]
	pub r#ref: String,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub created_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_group_silenced_refs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroupSilencedRef {
	pub server_group_id: Uuid,
	pub source: String,
	#[diesel(column_name = ref_)]
	#[serde(rename = "ref")]
	pub r#ref: String,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	pub created_by: Option<String>,
}

/// Does either the server-scope or group-scope silence list contain
/// `(source, ref)` for this server? `group_id` is the server's current
/// group; pass `None` if the server is ungrouped (and so can't be silenced
/// at group scope).
pub async fn is_silenced(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Option<Uuid>,
	source: &str,
	r#ref: &str,
) -> Result<bool> {
	use crate::schema::{server_group_silenced_refs, server_silenced_refs};

	let server_hit: i64 = server_silenced_refs::table
		.filter(
			server_silenced_refs::server_id
				.eq(server_id)
				.and(server_silenced_refs::source.eq(source))
				.and(server_silenced_refs::ref_.eq(r#ref)),
		)
		.count()
		.get_result(db)
		.await
		.map_err(AppError::from)?;
	if server_hit > 0 {
		return Ok(true);
	}

	let Some(gid) = group_id else {
		return Ok(false);
	};

	let group_hit: i64 = server_group_silenced_refs::table
		.filter(
			server_group_silenced_refs::server_group_id
				.eq(gid)
				.and(server_group_silenced_refs::source.eq(source))
				.and(server_group_silenced_refs::ref_.eq(r#ref)),
		)
		.count()
		.get_result(db)
		.await
		.map_err(AppError::from)?;
	Ok(group_hit > 0)
}

impl ServerSilencedRef {
	/// Add a server-scoped silence and re-evaluate any currently-open
	/// matching issues so they leave their incident. Idempotent: a
	/// duplicate (`server_id`, `source`, `ref`) is a no-op (the
	/// existing row's metadata is preserved).
	pub async fn add(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		source: &str,
		r#ref: &str,
		created_by: Option<&str>,
	) -> Result<Self> {
		use crate::schema::server_silenced_refs;

		let row: Self = diesel::insert_into(server_silenced_refs::table)
			.values((
				server_silenced_refs::server_id.eq(server_id),
				server_silenced_refs::source.eq(source),
				server_silenced_refs::ref_.eq(r#ref),
				server_silenced_refs::created_by.eq(created_by),
			))
			.on_conflict((
				server_silenced_refs::server_id,
				server_silenced_refs::source,
				server_silenced_refs::ref_,
			))
			.do_update()
			// no-op update so we can RETURNING the existing row
			.set(server_silenced_refs::server_id.eq(server_id))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)?;

		reevaluate_open_issues_for_server_ref(db, server_id, source, r#ref).await?;
		Ok(row)
	}

	/// Remove a server-scoped silence and re-evaluate any currently-open
	/// matching issues so they (re)join an incident if eligible.
	pub async fn remove(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		source: &str,
		r#ref: &str,
	) -> Result<()> {
		use crate::schema::server_silenced_refs;

		diesel::delete(
			server_silenced_refs::table.filter(
				server_silenced_refs::server_id
					.eq(server_id)
					.and(server_silenced_refs::source.eq(source))
					.and(server_silenced_refs::ref_.eq(r#ref)),
			),
		)
		.execute(db)
		.await
		.map_err(AppError::from)?;

		reevaluate_open_issues_for_server_ref(db, server_id, source, r#ref).await?;
		Ok(())
	}

	pub async fn list_for_server(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::server_silenced_refs::dsl;
		dsl::server_silenced_refs
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server_id))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}
}

impl ServerGroupSilencedRef {
	pub async fn add(
		db: &mut AsyncPgConnection,
		server_group_id: Uuid,
		source: &str,
		r#ref: &str,
		created_by: Option<&str>,
	) -> Result<Self> {
		use crate::schema::server_group_silenced_refs;

		let row: Self = diesel::insert_into(server_group_silenced_refs::table)
			.values((
				server_group_silenced_refs::server_group_id.eq(server_group_id),
				server_group_silenced_refs::source.eq(source),
				server_group_silenced_refs::ref_.eq(r#ref),
				server_group_silenced_refs::created_by.eq(created_by),
			))
			.on_conflict((
				server_group_silenced_refs::server_group_id,
				server_group_silenced_refs::source,
				server_group_silenced_refs::ref_,
			))
			.do_update()
			.set(server_group_silenced_refs::server_group_id.eq(server_group_id))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)?;

		reevaluate_open_issues_for_group_ref(db, server_group_id, source, r#ref).await?;
		Ok(row)
	}

	pub async fn remove(
		db: &mut AsyncPgConnection,
		server_group_id: Uuid,
		source: &str,
		r#ref: &str,
	) -> Result<()> {
		use crate::schema::server_group_silenced_refs;

		diesel::delete(
			server_group_silenced_refs::table.filter(
				server_group_silenced_refs::server_group_id
					.eq(server_group_id)
					.and(server_group_silenced_refs::source.eq(source))
					.and(server_group_silenced_refs::ref_.eq(r#ref)),
			),
		)
		.execute(db)
		.await
		.map_err(AppError::from)?;

		reevaluate_open_issues_for_group_ref(db, server_group_id, source, r#ref).await?;
		Ok(())
	}

	pub async fn list_for_group(
		db: &mut AsyncPgConnection,
		server_group_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::server_group_silenced_refs::dsl;
		dsl::server_group_silenced_refs
			.select(Self::as_select())
			.filter(dsl::server_group_id.eq(server_group_id))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}
}
