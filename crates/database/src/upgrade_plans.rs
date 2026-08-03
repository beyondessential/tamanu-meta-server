//! Where each deployment is going: the version a group intends to move to, and
//! optionally when.
//!
//! A plan is a statement of intent. Nothing here performs or schedules an
//! upgrade; the date is presentational and Canopy decides a plan is met by
//! watching what the group reports running.

use commons_errors::{AppError, Result};
use commons_types::version::VersionStr;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{server_groups::ServerGroup, versions::Version};

/// A group's recorded intention to move to a version.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::upgrade_plans)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpgradePlan {
	/// Unique identifier for this plan.
	pub id: Uuid,
	/// The group that intends to move.
	pub group_id: Uuid,
	/// The version it intends to move to.
	pub target_version_id: Uuid,
	/// The day the upgrade is expected, where one is known.
	#[diesel(deserialize_as = jiff_diesel::NullableDate, serialize_as = jiff_diesel::NullableDate)]
	#[schema(value_type = Option<String>)]
	pub planned_for: Option<Date>,
	/// Whatever the operator needs the next reader to know.
	pub note: Option<String>,
	/// The operator who recorded it.
	pub created_by: Option<String>,
	/// When it was recorded.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	#[schema(value_type = String)]
	pub created_at: Timestamp,
	/// When the group's reported version reached the target.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	#[schema(value_type = Option<String>)]
	pub met_at: Option<Timestamp>,
	/// When a newer plan replaced this one.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	#[schema(value_type = Option<String>)]
	pub superseded_at: Option<Timestamp>,
	/// The operator who last amended the date or note, where one has.
	pub amended_by: Option<String>,
	/// When it was last amended.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	#[schema(value_type = Option<String>)]
	pub amended_at: Option<Timestamp>,
	/// When the plan was withdrawn, if it was.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	#[schema(value_type = Option<String>)]
	pub withdrawn_at: Option<Timestamp>,
	/// The operator who withdrew it.
	pub withdrawn_by: Option<String>,
}

impl UpgradePlan {
	/// Record where a group is going, retiring any plan it already had.
	///
	/// The target must be published and ahead of what the group runs: a plan to
	/// move somewhere the deployment has already been is not a plan.
	// spec: UPG#a-plan
	pub async fn record(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		target_version_id: Uuid,
		planned_for: Option<Date>,
		note: Option<&str>,
		created_by: &str,
	) -> Result<Self> {
		use crate::schema::upgrade_plans::dsl;

		let target = Version::get_by_id(db, target_version_id).await?;
		if target.status != commons_types::version::VersionStatus::Published {
			return Err(AppError::BadRequest(
				"an unpublished version cannot be planned for".into(),
			));
		}
		if let Some(running) = ServerGroup::get_by_id(db, group_id)
			.await?
			.effective_version
			.clone() && target.as_semver() <= running.0
		{
			return Err(AppError::BadRequest(format!(
				"this group already runs {running}, which is not behind {}",
				target.as_semver()
			)));
		}

		diesel::update(dsl::upgrade_plans)
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::met_at.is_null())
			.filter(dsl::superseded_at.is_null())
			.filter(dsl::withdrawn_at.is_null())
			.set(dsl::superseded_at.eq(diesel::dsl::now))
			.execute(db)
			.await?;

		diesel::insert_into(dsl::upgrade_plans)
			.values((
				dsl::group_id.eq(group_id),
				dsl::target_version_id.eq(target_version_id),
				dsl::planned_for.eq(planned_for.map(jiff_diesel::Date::from)),
				dsl::note.eq(note),
				dsl::created_by.eq(created_by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(|e| match e {
				diesel::result::Error::DatabaseError(
					diesel::result::DatabaseErrorKind::UniqueViolation,
					_,
				) => AppError::Conflict(
					"another plan was recorded for this group at the same time".into(),
				),
				e => AppError::from(e),
			})
	}

	/// The group's open plan, if it has one.
	// spec: UPG#a-plan
	pub async fn open_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Option<Self>> {
		use crate::schema::upgrade_plans::dsl;

		dsl::upgrade_plans
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::met_at.is_null())
			.filter(dsl::superseded_at.is_null())
			.filter(dsl::withdrawn_at.is_null())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Every plan a group has had, newest first.
	// spec: UPG#when-a-plan-is-met
	pub async fn history_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::upgrade_plans::dsl;

		dsl::upgrade_plans
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Every open plan across the fleet, newest first.
	// spec: UPG#the-dashboard
	pub async fn all_open(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::upgrade_plans::dsl;

		dsl::upgrade_plans
			.select(Self::as_select())
			.filter(dsl::met_at.is_null())
			.filter(dsl::superseded_at.is_null())
			.filter(dsl::withdrawn_at.is_null())
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Amend an open plan's date and note.
	///
	/// The same plan better described, so it is not superseded and does not
	/// enter the history as a second plan. Changing where a deployment is going
	/// is a replacement instead, so the target is not amendable here.
	// spec: UPG#a-plan
	pub async fn amend(
		db: &mut AsyncPgConnection,
		id: Uuid,
		planned_for: Option<Date>,
		note: Option<&str>,
		amended_by: &str,
	) -> Result<Self> {
		use crate::schema::upgrade_plans::dsl;

		diesel::update(dsl::upgrade_plans)
			.filter(dsl::id.eq(id))
			.filter(dsl::met_at.is_null())
			.filter(dsl::superseded_at.is_null())
			.filter(dsl::withdrawn_at.is_null())
			.set((
				dsl::planned_for.eq(planned_for.map(jiff_diesel::Date::from)),
				dsl::note.eq(note),
				dsl::amended_by.eq(amended_by),
				dsl::amended_at.eq(diesel::dsl::now),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.optional()?
			.ok_or_else(|| AppError::BadRequest("only an open plan can be amended".into()))
	}

	/// Withdraw a plan: the deployment is no longer going there.
	///
	/// The plan is retained. Where a deployment was going and the fact that it
	/// stopped going there is what the history exists to record, and a withdrawn
	/// plan reads differently from one that was met.
	// spec: UPG#a-plan
	pub async fn withdraw(
		db: &mut AsyncPgConnection,
		id: Uuid,
		withdrawn_by: &str,
	) -> Result<Option<Self>> {
		use crate::schema::upgrade_plans::dsl;

		diesel::update(dsl::upgrade_plans)
			.filter(dsl::id.eq(id))
			.filter(dsl::met_at.is_null())
			.filter(dsl::superseded_at.is_null())
			.filter(dsl::withdrawn_at.is_null())
			.set((
				dsl::withdrawn_at.eq(diesel::dsl::now),
				dsl::withdrawn_by.eq(withdrawn_by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.optional()
			.map_err(AppError::from)
	}
}

/// Close every open plan whose group has reached its target, returning how many
/// were closed.
///
/// Reaching a version past the target closes the plan too: a deployment that
/// jumped further has done the upgrade and then some, and holding the plan open
/// would report it as outstanding.
// spec: UPG#when-a-plan-is-met
pub async fn close_met_plans(db: &mut AsyncPgConnection) -> Result<usize> {
	use crate::schema::upgrade_plans::dsl;

	let mut closed = 0;
	for plan in UpgradePlan::all_open(db).await? {
		let Some(running) = ServerGroup::get_by_id(db, plan.group_id)
			.await?
			.effective_version
			.clone()
		else {
			continue;
		};
		let target = Version::get_by_id(db, plan.target_version_id).await?;
		if running.0 < target.as_semver() {
			continue;
		}

		diesel::update(dsl::upgrade_plans)
			.filter(dsl::id.eq(plan.id))
			.set(dsl::met_at.eq(diesel::dsl::now))
			.execute(db)
			.await?;
		closed += 1;
	}

	Ok(closed)
}

/// The version `group` plans to move to, if it has an open plan.
///
/// This is what pre-upgrade testing targets in preference to the newest
/// published version, so a deployment deliberately moving to an older minor is
/// held against that minor instead.
// spec: UPG#what-reads-a-plan
pub async fn planned_target(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Option<Version>> {
	let Some(plan) = UpgradePlan::open_for_group(db, group_id).await? else {
		return Ok(None);
	};
	let target = Version::get_by_id(db, plan.target_version_id).await?;
	// A target yanked after the plan was recorded has no artefacts to fetch, so
	// it cannot steer testing; the plan stays open for the operator to revisit.
	if target.status != commons_types::version::VersionStatus::Published {
		return Ok(None);
	}
	Ok(Some(target))
}

/// Whether an open plan's date has passed without it being met.
///
/// Presentational only: an upgrade slipping is normal, and a date someone typed
/// is no basis for treating anything as failed.
// spec: UPG#the-dashboard
pub fn is_late(plan: &UpgradePlan, today: Date) -> bool {
	plan.met_at.is_none()
		&& plan.superseded_at.is_none()
		&& plan.withdrawn_at.is_none()
		&& plan.planned_for.is_some_and(|date| date < today)
}

/// A plan's target rendered as semver, for display beside what the group runs.
pub async fn target_version_str(
	db: &mut AsyncPgConnection,
	plan: &UpgradePlan,
) -> Result<VersionStr> {
	Version::get_by_id(db, plan.target_version_id)
		.await
		.map(|version| VersionStr(version.as_semver()))
}
