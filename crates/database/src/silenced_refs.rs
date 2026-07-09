//! Operator-managed silences, stored as scoped check policies.
//!
//! A silence is a scoped transform with a `skipped` ceiling (see
//! [`crate::check_policies::ScopedCheckPolicy`]): the matching check
//! keeps recording its observed results, but its effective result is
//! skipped, so it raises nothing and counts nowhere —
//! [`crate::issues::re_evaluate_incident_membership`] treats it as a
//! "should leave" reason, and the health rollups drop it.
//!
//! This module keeps the historical (source, ref) surface the private
//! API and UI speak: refs carry the `health/` namespace prefix for
//! source-reported checks, while the scoped-policy storage is keyed by
//! bare check name. The mapping is applied on the way in and out.

use std::collections::{BTreeSet, HashMap};

use commons_errors::Result;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::check_policies::{PolicyScope, ScopedCheckPolicy};
use crate::issues::{
	MANUAL_SOURCE, reevaluate_open_issues_for_group_ref, reevaluate_open_issues_for_server_ref,
};
use crate::statuses::CANOPY_SOURCE;

/// The ref prefix (with trailing separator) healthcheck issues use,
/// whichever source reports them. Mirrors the public-server's `HEALTH_REF`.
const HEALTH_REF_PREFIX: &str = "health/";

/// The check name a silence ref maps to: refs of source-reported checks
/// carry the `health/` namespace prefix, canopy/manual refs are already
/// bare check names.
fn ref_to_check(r#ref: &str) -> &str {
	r#ref.strip_prefix(HEALTH_REF_PREFIX).unwrap_or(r#ref)
}

/// The ref a silenced check name presents as: reserved sources file at
/// bare refs, everything else under the `health/` namespace.
fn check_to_ref(source: &str, check: &str) -> String {
	if source == CANOPY_SOURCE || source == MANUAL_SOURCE {
		check.to_string()
	} else {
		format!("{HEALTH_REF_PREFIX}{check}")
	}
}

/// A silenced issue reference scoped to a single server: issues matching
/// this `(source, ref)` on this server are still recorded, but are excluded
/// from incidents and notifications.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerSilencedRef {
	/// The server this silence applies to.
	pub server_id: Uuid,
	/// The issue source this silence matches.
	pub source: String,
	/// The issue reference this silence matches.
	#[serde(rename = "ref")]
	pub r#ref: String,
	/// When this silence was created.
	pub created_at: Timestamp,
	/// The operator who created this silence. `None` if not recorded.
	pub created_by: Option<String>,
}

/// A silenced issue reference scoped to an entire server group: issues
/// matching this `(source, ref)` on any server in the group (or raised
/// directly against the group) are still recorded, but are excluded from
/// incidents and notifications.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerGroupSilencedRef {
	/// The server group this silence applies to.
	pub server_group_id: Uuid,
	/// The issue source this silence matches.
	pub source: String,
	/// The issue reference this silence matches.
	#[serde(rename = "ref")]
	pub r#ref: String,
	/// When this silence was created.
	pub created_at: Timestamp,
	/// The operator who created this silence. `None` if not recorded.
	pub created_by: Option<String>,
}

/// Is a silence in force for `(source, ref)` on this server, at either
/// server or group scope? `group_id` is the server's current group; pass
/// `None` if the server is ungrouped (and so can't be silenced at group
/// scope).
pub async fn is_silenced(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Option<Uuid>,
	source: &str,
	r#ref: &str,
) -> Result<bool> {
	let check = ref_to_check(r#ref);
	let is_silence =
		|p: Option<ScopedCheckPolicy>| p.is_some_and(|p| p.ceiling.as_deref() == Some("skipped"));
	if is_silence(ScopedCheckPolicy::get(db, PolicyScope::Server(server_id), source, check).await?)
	{
		return Ok(true);
	}
	let Some(gid) = group_id else {
		return Ok(false);
	};
	Ok(is_silence(
		ScopedCheckPolicy::get(db, PolicyScope::Group(gid), source, check).await?,
	))
}

/// All refs silenced for this server under any source and starting with
/// `ref_prefix`, combining the server's own silences with its group's.
/// `group_id` is the server's current group; pass `None` if the server is
/// ungrouped. Used to build the device-facing effective check-severity map.
/// May contain duplicates when a ref is silenced at both scopes.
pub async fn silenced_refs_with_prefix(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Option<Uuid>,
	ref_prefix: &str,
) -> Result<Vec<String>> {
	let mut refs: Vec<String> =
		ScopedCheckPolicy::list_silences(db, PolicyScope::Server(server_id))
			.await?
			.into_iter()
			.map(|p| check_to_ref(&p.source, &p.check_name))
			.collect();
	if let Some(gid) = group_id {
		refs.extend(
			ScopedCheckPolicy::list_silences(db, PolicyScope::Group(gid))
				.await?
				.into_iter()
				.map(|p| check_to_ref(&p.source, &p.check_name)),
		);
	}
	refs.retain(|r| r.starts_with(ref_prefix));
	Ok(refs)
}

/// Healthcheck names silenced for each of the given `(server, group)`
/// pairs, at either scope, whichever source they were silenced under.
/// Pass each server's current group id (`None` for ungrouped). One batch
/// query regardless of how many servers are asked about. Servers with no
/// applicable silences are absent from the map.
///
/// This feeds [`crate::statuses::Status::health_state_ignoring`]: a
/// silenced check keeps recording results but is presented as skipped
/// and doesn't count toward the server's health rollup.
pub async fn silenced_health_checks_for_servers(
	db: &mut AsyncPgConnection,
	servers: &[(Uuid, Option<Uuid>)],
) -> Result<HashMap<Uuid, BTreeSet<String>>> {
	use crate::schema::scoped_check_policies::dsl;

	let mut out: HashMap<Uuid, BTreeSet<String>> = HashMap::new();
	if servers.is_empty() {
		return Ok(out);
	}

	let server_ids: Vec<Uuid> = servers.iter().map(|(id, _)| *id).collect();
	let group_ids: Vec<Uuid> = servers
		.iter()
		.filter_map(|(_, group)| *group)
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();

	// Health rollups match by check name across every reporting source;
	// canopy/manual silences aren't source-reported checks and don't
	// belong in the ignore-set.
	let rows: Vec<(Option<Uuid>, Option<Uuid>, String)> = dsl::scoped_check_policies
		.select((dsl::server_id, dsl::server_group_id, dsl::check_name))
		.filter(dsl::ceiling.eq("skipped"))
		.filter(dsl::source.ne_all([CANOPY_SOURCE, MANUAL_SOURCE]))
		.filter(
			dsl::server_id
				.eq_any(&server_ids)
				.or(dsl::server_group_id.eq_any(&group_ids)),
		)
		.load(db)
		.await?;

	let mut by_group: HashMap<Uuid, Vec<String>> = HashMap::new();
	for (server_id, group_id, check) in rows {
		if let Some(sid) = server_id {
			out.entry(sid).or_default().insert(check);
		} else if let Some(gid) = group_id {
			by_group.entry(gid).or_default().push(check);
		}
	}
	for (server_id, group_id) in servers {
		if let Some(checks) = group_id.as_ref().and_then(|g| by_group.get(g)) {
			out.entry(*server_id)
				.or_default()
				.extend(checks.iter().cloned());
		}
	}

	Ok(out)
}

/// Single-server variant of [`silenced_health_checks_for_servers`].
/// `group_id` is the server's current group; pass `None` if ungrouped.
pub async fn silenced_health_checks_for_server(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	group_id: Option<Uuid>,
) -> Result<BTreeSet<String>> {
	Ok(
		silenced_health_checks_for_servers(db, &[(server_id, group_id)])
			.await?
			.remove(&server_id)
			.unwrap_or_default(),
	)
}

impl ServerSilencedRef {
	fn from_policy(p: ScopedCheckPolicy) -> Option<Self> {
		Some(Self {
			server_id: p.server_id?,
			r#ref: check_to_ref(&p.source, &p.check_name),
			source: p.source,
			created_at: p.created_at,
			created_by: p.created_by,
		})
	}

	/// Add a server-scoped silence and re-evaluate any currently-open
	/// matching issues so they leave their incident. Idempotent.
	pub async fn add(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		source: &str,
		r#ref: &str,
		created_by: Option<&str>,
	) -> Result<Self> {
		let policy = ScopedCheckPolicy::silence(
			db,
			PolicyScope::Server(server_id),
			source,
			ref_to_check(r#ref),
			created_by,
		)
		.await?;
		reevaluate_open_issues_for_server_ref(db, server_id, source, r#ref).await?;
		Ok(Self::from_policy(policy).expect("server-scoped silence has a server_id"))
	}

	/// Remove a server-scoped silence and re-evaluate any currently-open
	/// matching issues so they (re)join an incident if eligible.
	pub async fn remove(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		source: &str,
		r#ref: &str,
	) -> Result<()> {
		ScopedCheckPolicy::unsilence(
			db,
			PolicyScope::Server(server_id),
			source,
			ref_to_check(r#ref),
		)
		.await?;
		reevaluate_open_issues_for_server_ref(db, server_id, source, r#ref).await?;
		Ok(())
	}

	pub async fn list_for_server(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Vec<Self>> {
		Ok(
			ScopedCheckPolicy::list_silences(db, PolicyScope::Server(server_id))
				.await?
				.into_iter()
				.filter_map(Self::from_policy)
				.collect(),
		)
	}
}

impl ServerGroupSilencedRef {
	fn from_policy(p: ScopedCheckPolicy) -> Option<Self> {
		Some(Self {
			server_group_id: p.server_group_id?,
			r#ref: check_to_ref(&p.source, &p.check_name),
			source: p.source,
			created_at: p.created_at,
			created_by: p.created_by,
		})
	}

	pub async fn add(
		db: &mut AsyncPgConnection,
		server_group_id: Uuid,
		source: &str,
		r#ref: &str,
		created_by: Option<&str>,
	) -> Result<Self> {
		let policy = ScopedCheckPolicy::silence(
			db,
			PolicyScope::Group(server_group_id),
			source,
			ref_to_check(r#ref),
			created_by,
		)
		.await?;
		reevaluate_open_issues_for_group_ref(db, server_group_id, source, r#ref).await?;
		Ok(Self::from_policy(policy).expect("group-scoped silence has a server_group_id"))
	}

	pub async fn remove(
		db: &mut AsyncPgConnection,
		server_group_id: Uuid,
		source: &str,
		r#ref: &str,
	) -> Result<()> {
		ScopedCheckPolicy::unsilence(
			db,
			PolicyScope::Group(server_group_id),
			source,
			ref_to_check(r#ref),
		)
		.await?;
		reevaluate_open_issues_for_group_ref(db, server_group_id, source, r#ref).await?;
		Ok(())
	}

	pub async fn list_for_group(
		db: &mut AsyncPgConnection,
		server_group_id: Uuid,
	) -> Result<Vec<Self>> {
		Ok(
			ScopedCheckPolicy::list_silences(db, PolicyScope::Group(server_group_id))
				.await?
				.into_iter()
				.filter_map(Self::from_policy)
				.collect(),
		)
	}
}
