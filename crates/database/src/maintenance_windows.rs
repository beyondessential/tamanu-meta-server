//! Operator declarations that a server or a group is being worked on.
//!
//! While a window suspends a target, every check on it grades to skipped
//! (the transform a silence applies to one check, applied to all of them:
//! see [`crate::check_policies::ScopedCheckPolicy::chain_for`]), so nothing
//! on the target opens or joins an incident and nothing notifies.
//!
//! Suspension outlasts the window itself by [`SETTLE`]. A server is back
//! before the sources on it have reported again, and a server whose every
//! source is stale is unreachable, so ending suspension the instant the
//! work finishes would report a deployment that has just come back as
//! failed for as long as the work took.

use std::collections::HashSet;

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::issues::Scope;
use crate::server_groups::ServerGroup;
use crate::servers::Server;
use crate::slack_outbox::{KIND_MAINTENANCE_DECLARED, KIND_MAINTENANCE_ENDED, SlackOutbox, vars};

/// How long suspension outlasts the window, giving the reporters on a
/// server time to be heard from before Canopy judges them. The same for
/// every window.
pub const SETTLE: SignedDuration = SignedDuration::from_mins(10);

/// A declaration that a server or a group is being worked on.
#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::maintenance_windows)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MaintenanceWindow {
	/// Unique identifier of this window.
	pub id: Uuid,
	/// Set for a window over one server.
	pub server_id: Option<Uuid>,
	/// Set for a window over a group, covering the group's own checks and
	/// those of every server in it.
	pub server_group_id: Option<Uuid>,
	/// When the operator expects the work to finish. A window reaching it
	/// ends itself.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub expected_end: Timestamp,
	/// What is being done, where the operator said.
	pub note: Option<String>,
	/// The operator who declared the window. `None` if not recorded.
	pub declared_by: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	/// When the window was declared.
	pub declared_at: Timestamp,
	/// The operator who last amended the window, where one has.
	pub amended_by: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	/// When the window was last amended.
	pub amended_at: Option<Timestamp>,
	/// When the window stopped holding. `None` while it holds.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub ended_at: Option<Timestamp>,
	/// The operator who lifted the window. `None` where its expected end
	/// passed instead.
	pub ended_by: Option<String>,
	/// Stamped once the settle period has elapsed and the target's issues
	/// have been re-evaluated.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub settled_at: Option<Timestamp>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	/// When this record was created.
	pub created_at: Timestamp,
	/// When this record was last modified.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

impl MaintenanceWindow {
	/// The target this window covers.
	pub fn scope(&self) -> Scope {
		Scope::from_columns(self.server_id, self.server_group_id)
	}

	/// When the window itself ended, or is due to: an operator's lift where
	/// there was one, the expected end otherwise.
	pub fn window_end(&self) -> Timestamp {
		self.ended_at.unwrap_or(self.expected_end)
	}

	/// When suspension over the target ends.
	pub fn suspension_end(&self) -> Timestamp {
		self.window_end() + SETTLE
	}

	/// Is the window still holding, as opposed to settling or over?
	pub fn holds_at(&self, now: Timestamp) -> bool {
		self.ended_at.is_none() && now < self.expected_end
	}

	/// Is the target still suspended, whether the window holds or is
	/// settling?
	pub fn suspends_at(&self, now: Timestamp) -> bool {
		now < self.suspension_end()
	}

	/// Declare a window over `scope`, or amend the open one if the target
	/// already has it: a target has at most one open window.
	pub async fn declare(
		db: &mut AsyncPgConnection,
		scope: Scope,
		expected_end: Timestamp,
		note: Option<&str>,
		by: Option<&str>,
	) -> Result<Self> {
		use crate::schema::maintenance_windows::dsl;
		let Some((server, group)) = fleet_columns(scope) else {
			return Err(AppError::BadRequest(
				"a maintenance window covers a server or a group".into(),
			));
		};
		if expected_end <= Timestamp::now() {
			return Err(AppError::BadRequest(
				"a maintenance window ends in the future".into(),
			));
		}

		if let Some(open) = Self::open_for(db, scope).await? {
			return diesel::update(dsl::maintenance_windows.filter(dsl::id.eq(open.id)))
				.set((
					dsl::expected_end.eq(jiff_diesel::Timestamp::from(expected_end)),
					dsl::note.eq(note),
					dsl::amended_by.eq(by),
					dsl::amended_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
					dsl::updated_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				))
				.returning(Self::as_select())
				.get_result(db)
				.await
				.map_err(AppError::from);
		}

		let window: Self = diesel::insert_into(dsl::maintenance_windows)
			.values((
				dsl::server_id.eq(server),
				dsl::server_group_id.eq(group),
				dsl::expected_end.eq(jiff_diesel::Timestamp::from(expected_end)),
				dsl::note.eq(note),
				dsl::declared_by.eq(by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)?;

		let label = target_label(db, scope).await?;
		SlackOutbox::enqueue(
			db,
			KIND_MAINTENANCE_DECLARED,
			None,
			None,
			None,
			vars::maintenance_declared(&label, by, &format_when(expected_end), note),
			Timestamp::now(),
		)
		.await?;

		// The target's issues leave their incident now, rather than waiting
		// for each check to be re-graded on its next report.
		let closed_because = match by {
			Some(login) => format!("maintenance declared by {login}"),
			None => "maintenance being declared".to_string(),
		};
		crate::issues::reevaluate_open_issues_for_scope(db, scope, Some(&closed_because)).await?;

		Ok(window)
	}

	/// Lift a window before its expected end. A window already ended is
	/// returned untouched, so a double lift is idempotent.
	pub async fn lift(db: &mut AsyncPgConnection, id: Uuid, by: Option<&str>) -> Result<Self> {
		use crate::schema::maintenance_windows::dsl;
		let now = Timestamp::now();
		let lifted: Option<Self> = diesel::update(
			dsl::maintenance_windows
				.filter(dsl::id.eq(id))
				.filter(dsl::ended_at.is_null()),
		)
		.set((
			dsl::ended_at.eq(jiff_diesel::Timestamp::from(now)),
			dsl::ended_by.eq(by),
			dsl::updated_at.eq(jiff_diesel::Timestamp::from(now)),
		))
		.returning(Self::as_select())
		.get_result(db)
		.await
		.optional()
		.map_err(AppError::from)?;
		match lifted {
			Some(window) => {
				window.announce_ended(db).await?;
				Ok(window)
			}
			None => Self::get(db, id).await,
		}
	}

	/// Tell operators the target is being watched again from the end of its
	/// settle period. Whether an operator lifted the window or its expected
	/// end passed is what `ended_by` records.
	async fn announce_ended(&self, db: &mut AsyncPgConnection) -> Result<()> {
		let label = target_label(db, self.scope()).await?;
		SlackOutbox::enqueue(
			db,
			KIND_MAINTENANCE_ENDED,
			None,
			None,
			None,
			vars::maintenance_ended(
				&label,
				self.ended_by.as_deref(),
				&format_when(self.suspension_end()),
			),
			Timestamp::now(),
		)
		.await?;
		Ok(())
	}

	pub async fn get(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		use crate::schema::maintenance_windows::dsl;
		dsl::maintenance_windows
			.select(Self::as_select())
			.filter(dsl::id.eq(id))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// The target's window while it holds. A settling window is over as far
	/// as declaring goes: a fresh declaration opens a new one.
	pub async fn open_for(db: &mut AsyncPgConnection, scope: Scope) -> Result<Option<Self>> {
		use crate::schema::maintenance_windows::dsl;
		let Some((server, group)) = fleet_columns(scope) else {
			return Ok(None);
		};
		dsl::maintenance_windows
			.select(Self::as_select())
			.filter(
				dsl::ended_at
					.is_null()
					.and(dsl::server_id.is_not_distinct_from(server))
					.and(dsl::server_group_id.is_not_distinct_from(group)),
			)
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// The open windows over an environment: the group's own and any over the
	/// given servers. A window past its expected end stays open until the
	/// sweep stamps it, so pair with [`Self::holds_at`].
	pub async fn open_over(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		server_ids: &[Uuid],
	) -> Result<Vec<Self>> {
		use crate::schema::maintenance_windows::dsl;
		dsl::maintenance_windows
			.select(Self::as_select())
			.filter(dsl::ended_at.is_null())
			.filter(
				dsl::server_group_id
					.eq(group_id)
					.or(dsl::server_id.eq_any(server_ids)),
			)
			.order(dsl::declared_at.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Every window still holding, most recently declared first.
	pub async fn list_open(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::maintenance_windows::dsl;
		dsl::maintenance_windows
			.select(Self::as_select())
			.filter(dsl::ended_at.is_null())
			.order(dsl::declared_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The target's windows, open and ended, most recently declared first.
	pub async fn list_for_scope(
		db: &mut AsyncPgConnection,
		scope: Scope,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::maintenance_windows::dsl;
		let Some((server, group)) = fleet_columns(scope) else {
			return Ok(Vec::new());
		};
		dsl::maintenance_windows
			.select(Self::as_select())
			.filter(
				dsl::server_id
					.is_not_distinct_from(server)
					.and(dsl::server_group_id.is_not_distinct_from(group)),
			)
			.order(dsl::declared_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Is a check filed against `(server_id, group_id)` suspended? A server
	/// is covered by its own window and by its group's, and stays suspended
	/// until the last of them has settled.
	pub async fn suspends(
		db: &mut AsyncPgConnection,
		server_id: Option<Uuid>,
		group_id: Option<Uuid>,
	) -> Result<bool> {
		use crate::schema::maintenance_windows::dsl;
		if server_id.is_none() && group_id.is_none() {
			return Ok(false);
		}
		let cutoff = jiff_diesel::Timestamp::from(Timestamp::now() - SETTLE);
		let found: Option<Uuid> = dsl::maintenance_windows
			.select(dsl::id)
			.filter(
				dsl::server_id
					.eq(server_id)
					.or(dsl::server_group_id.eq(group_id)),
			)
			.filter(
				dsl::ended_at
					.is_null()
					.and(dsl::expected_end.gt(cutoff))
					.or(dsl::ended_at.gt(cutoff)),
			)
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?;
		Ok(found.is_some())
	}

	/// The servers and groups currently suspended, for callers judging many
	/// targets in one pass.
	pub async fn suspended_targets(
		db: &mut AsyncPgConnection,
	) -> Result<(HashSet<Uuid>, HashSet<Uuid>)> {
		use crate::schema::maintenance_windows::dsl;
		let cutoff = jiff_diesel::Timestamp::from(Timestamp::now() - SETTLE);
		let rows: Vec<(Option<Uuid>, Option<Uuid>)> = dsl::maintenance_windows
			.select((dsl::server_id, dsl::server_group_id))
			.filter(
				dsl::ended_at
					.is_null()
					.and(dsl::expected_end.gt(cutoff))
					.or(dsl::ended_at.gt(cutoff)),
			)
			.load(db)
			.await
			.map_err(AppError::from)?;
		let mut servers = HashSet::new();
		let mut groups = HashSet::new();
		for (server, group) in rows {
			if let Some(id) = server {
				servers.insert(id);
			}
			if let Some(id) = group {
				groups.insert(id);
			}
		}
		Ok((servers, groups))
	}

	/// End every window whose expected end has passed, stamping the end at
	/// that expected end rather than at now: the window was over then, and
	/// backdating keeps the settle period honest however late this runs.
	/// Returns what it ended, for notification.
	pub async fn sweep_expired(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::maintenance_windows::dsl;
		let now = jiff_diesel::Timestamp::from(Timestamp::now());
		let expired: Vec<Self> = diesel::update(
			dsl::maintenance_windows
				.filter(dsl::ended_at.is_null())
				.filter(dsl::expected_end.le(now)),
		)
		.set((
			dsl::ended_at.eq(dsl::expected_end.nullable()),
			dsl::updated_at.eq(now),
		))
		.returning(Self::as_select())
		.get_results(db)
		.await
		.map_err(AppError::from)?;
		for window in &expired {
			window.announce_ended(db).await?;
		}
		Ok(expired)
	}

	/// End windows that have reached their expected end, then re-evaluate
	/// the targets whose settle period has since elapsed, so anything still
	/// degraded contributes again. Returns how many of each.
	///
	/// A target still covered by another window is unaffected by the second
	/// pass: membership re-evaluation consults the windows over it, so the
	/// last one to end is the one that lets its issues back in.
	pub async fn sweep(db: &mut AsyncPgConnection) -> Result<(usize, usize)> {
		let ended = Self::sweep_expired(db).await?;
		let settled = Self::claim_settled(db).await?;
		for window in &settled {
			crate::issues::reevaluate_open_issues_for_scope(db, window.scope(), None).await?;
		}
		Ok((ended.len(), settled.len()))
	}

	/// Claim the ended windows whose settle period has elapsed, so their
	/// targets can be re-evaluated once each.
	pub async fn claim_settled(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::maintenance_windows::dsl;
		let now = Timestamp::now();
		let cutoff = jiff_diesel::Timestamp::from(now - SETTLE);
		diesel::update(
			dsl::maintenance_windows
				.filter(dsl::settled_at.is_null())
				.filter(dsl::ended_at.is_not_null())
				.filter(dsl::ended_at.le(cutoff)),
		)
		.set(dsl::settled_at.eq(jiff_diesel::Timestamp::from(now)))
		.returning(Self::as_select())
		.get_results(db)
		.await
		.map_err(AppError::from)
	}
}

/// How a window's target reads in a notification.
pub async fn target_label(db: &mut AsyncPgConnection, scope: Scope) -> Result<String> {
	match scope {
		Scope::Server(sid) => {
			let server = Server::get_by_id(db, sid).await?;
			match server.group_id {
				Some(gid) => {
					let group = ServerGroup::get_by_id(db, gid).await?;
					Ok(crate::issues::format_group_label(&group, Some(&server)))
				}
				None => Ok(server
					.name
					.clone()
					.or_else(|| server.host.as_ref().map(|h| h.0.to_string()))
					.unwrap_or_else(|| server.id.to_string())),
			}
		}
		Scope::Group(gid) => Ok(ServerGroup::get_by_id(db, gid).await?.name),
		Scope::Global => Ok("Canopy".to_string()),
	}
}

fn format_when(at: Timestamp) -> String {
	at.strftime("%Y-%m-%d %H:%M UTC").to_string()
}

/// The storage columns for a window's scope. Canopy-wide is not a target a
/// window covers: Canopy's own checks are its self-monitoring, and fleet
/// work never suspends them.
fn fleet_columns(scope: Scope) -> Option<(Option<Uuid>, Option<Uuid>)> {
	match scope {
		Scope::Server(id) => Some((Some(id), None)),
		Scope::Group(id) => Some((None, Some(id))),
		Scope::Global => None,
	}
}
