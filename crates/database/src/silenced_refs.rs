//! Operator-managed silence list for issue refs.
//!
//! A silenced `(source, ref)` tuple at server or group scope tells the
//! incident workflow to ignore the matching issues — they still record
//! (so the issue and event rows exist), but
//! [`crate::issues::re_evaluate_incident_membership`] treats them as a
//! "should leave" reason, the same way it treats snoozed or unmonitored.
//! Healthcheck silences (`(status, health/<check>)`) additionally drop the
//! check out of the server's health rollup — see
//! [`silenced_health_checks_for_servers`] and
//! [`crate::statuses::Status::health_state_ignoring`].
//!
//! Two sibling tables (`server_silenced_refs`, `server_group_silenced_refs`)
//! keep referential integrity tight without nullable FKs. A given issue is
//! silenced if either applies (server-scope wins for the server itself,
//! group-scope catches the whole group).

use std::collections::{BTreeSet, HashMap};

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::issues::{reevaluate_open_issues_for_group_ref, reevaluate_open_issues_for_server_ref};

/// The `source` the public-server status ingestion files healthcheck
/// issues under, so a silence on a healthcheck is `(status,
/// health/<check>)`. Mirrors the public-server's `STATUS_SOURCE`.
const STATUS_SOURCE: &str = "status";

/// The ref prefix (with trailing separator) healthcheck issues use
/// under [`STATUS_SOURCE`]. Mirrors the public-server's `HEALTH_REF`.
const HEALTH_REF_PREFIX: &str = "health/";

/// A silenced issue reference scoped to a single server: issues matching
/// this `(source, ref)` on this server are still recorded, but are excluded
/// from incidents and notifications.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_silenced_refs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerSilencedRef {
	/// The server this silence applies to.
	pub server_id: Uuid,
	/// The issue source this silence matches.
	pub source: String,
	/// The issue reference this silence matches.
	#[diesel(column_name = ref_)]
	#[serde(rename = "ref")]
	pub r#ref: String,
	/// When this silence was created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// The operator who created this silence. `None` if not recorded.
	pub created_by: Option<String>,
}

/// A silenced issue reference scoped to an entire server group: issues
/// matching this `(source, ref)` on any server in the group (or raised
/// directly against the group) are still recorded, but are excluded from
/// incidents and notifications.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_group_silenced_refs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroupSilencedRef {
	/// The server group this silence applies to.
	pub server_group_id: Uuid,
	/// The issue source this silence matches.
	pub source: String,
	/// The issue reference this silence matches.
	#[diesel(column_name = ref_)]
	#[serde(rename = "ref")]
	pub r#ref: String,
	/// When this silence was created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// The operator who created this silence. `None` if not recorded.
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

/// All refs silenced for this server under `source` and starting with
/// `ref_prefix`, combining the server's own silence list with its group's.
/// `group_id` is the server's current group; pass `None` if the server is
/// ungrouped. Used to build the device-facing effective check-severity map.
/// May contain duplicates when a ref is silenced at both scopes.
pub async fn silenced_refs_with_prefix(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Option<Uuid>,
	source: &str,
	ref_prefix: &str,
) -> Result<Vec<String>> {
	use crate::schema::{server_group_silenced_refs, server_silenced_refs};

	debug_assert!(
		!ref_prefix.contains(['%', '_', '\\']),
		"prefix is used in a LIKE pattern and must not contain wildcards"
	);

	let mut refs: Vec<String> = server_silenced_refs::table
		.select(server_silenced_refs::ref_)
		.filter(
			server_silenced_refs::server_id
				.eq(server_id)
				.and(server_silenced_refs::source.eq(source))
				.and(server_silenced_refs::ref_.like(format!("{ref_prefix}%"))),
		)
		.load(db)
		.await
		.map_err(AppError::from)?;

	if let Some(gid) = group_id {
		let group_refs: Vec<String> = server_group_silenced_refs::table
			.select(server_group_silenced_refs::ref_)
			.filter(
				server_group_silenced_refs::server_group_id
					.eq(gid)
					.and(server_group_silenced_refs::source.eq(source))
					.and(server_group_silenced_refs::ref_.like(format!("{ref_prefix}%"))),
			)
			.load(db)
			.await
			.map_err(AppError::from)?;
		refs.extend(group_refs);
	}

	Ok(refs)
}

/// Healthcheck names silenced for each of the given `(server, group)`
/// pairs, at either scope: the `<check>` of every `(status,
/// health/<check>)` silence entry that applies to the server. Pass each
/// server's current group id (`None` for ungrouped). Two batch queries,
/// one per scope table, regardless of how many servers are asked about.
/// Servers with no applicable silences are absent from the map.
///
/// This feeds [`crate::statuses::Status::health_state_ignoring`]: a
/// silenced check keeps recording results but is presented as skipped
/// and doesn't count toward the server's health rollup.
pub async fn silenced_health_checks_for_servers(
	db: &mut AsyncPgConnection,
	servers: &[(Uuid, Option<Uuid>)],
) -> Result<HashMap<Uuid, BTreeSet<String>>> {
	use crate::schema::{server_group_silenced_refs, server_silenced_refs};

	let mut out: HashMap<Uuid, BTreeSet<String>> = HashMap::new();
	if servers.is_empty() {
		return Ok(out);
	}

	let like_pattern = format!("{HEALTH_REF_PREFIX}%");

	let server_ids: Vec<Uuid> = servers.iter().map(|(id, _)| *id).collect();
	let server_rows: Vec<(Uuid, String)> = server_silenced_refs::table
		.select((server_silenced_refs::server_id, server_silenced_refs::ref_))
		.filter(server_silenced_refs::server_id.eq_any(&server_ids))
		.filter(server_silenced_refs::source.eq(STATUS_SOURCE))
		.filter(server_silenced_refs::ref_.like(&like_pattern))
		.load(db)
		.await
		.map_err(AppError::from)?;
	for (server_id, r#ref) in server_rows {
		if let Some(check) = r#ref.strip_prefix(HEALTH_REF_PREFIX) {
			out.entry(server_id).or_default().insert(check.to_string());
		}
	}

	let group_ids: Vec<Uuid> = servers
		.iter()
		.filter_map(|(_, group)| *group)
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	if !group_ids.is_empty() {
		let group_rows: Vec<(Uuid, String)> = server_group_silenced_refs::table
			.select((
				server_group_silenced_refs::server_group_id,
				server_group_silenced_refs::ref_,
			))
			.filter(server_group_silenced_refs::server_group_id.eq_any(&group_ids))
			.filter(server_group_silenced_refs::source.eq(STATUS_SOURCE))
			.filter(server_group_silenced_refs::ref_.like(&like_pattern))
			.load(db)
			.await
			.map_err(AppError::from)?;
		let mut by_group: HashMap<Uuid, Vec<String>> = HashMap::new();
		for (group_id, r#ref) in group_rows {
			if let Some(check) = r#ref.strip_prefix(HEALTH_REF_PREFIX) {
				by_group
					.entry(group_id)
					.or_default()
					.push(check.to_string());
			}
		}
		for (server_id, group_id) in servers {
			if let Some(checks) = group_id.as_ref().and_then(|g| by_group.get(g)) {
				out.entry(*server_id)
					.or_default()
					.extend(checks.iter().cloned());
			}
		}
	}

	Ok(out)
}

/// Single-server variant of [`silenced_health_checks_for_servers`],
/// via [`silenced_refs_with_prefix`]. `group_id` is the server's
/// current group; pass `None` if ungrouped.
pub async fn silenced_health_checks_for_server(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Option<Uuid>,
) -> Result<BTreeSet<String>> {
	let refs = silenced_refs_with_prefix(db, server_id, group_id, STATUS_SOURCE, HEALTH_REF_PREFIX)
		.await?;
	Ok(refs
		.iter()
		.filter_map(|r| r.strip_prefix(HEALTH_REF_PREFIX))
		.map(String::from)
		.collect())
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
