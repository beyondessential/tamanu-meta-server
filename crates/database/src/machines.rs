use commons_errors::{AppError, Result};
use commons_types::{geo::GeoPoint, server::TagMap};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::pg_duration::PgDuration;

/// A host in the fleet: a box, physical or virtual, that canopy monitors.
///
/// Distinct from the application server running on it. A machine carries the
/// facts that belong to the box — where it is, what identity speaks for it,
/// how long it may be silent — so a host running two workloads reports its
/// platform, memory and filesystems once rather than once per workload.
///
/// A machine hosts any number of applications, including none: one created but
/// not yet reporting presents as awaiting check-in rather than as an error.
// spec: FLT
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::machines)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Machine {
	/// Unique identifier for this machine.
	pub id: Uuid,
	/// The name its operator gave it. Distinct from the hostname the
	/// operating system reports, which is a reported figure rather than a
	/// field an operator sets.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	/// The group this machine belongs to. The one thing an operator supplies
	/// when creating a machine: which deployment a box belongs to is the one
	/// fact the box has no way of knowing. The applications on it take it.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub group_id: Option<Uuid>,
	/// The identity that authenticates this machine, if one is enrolled. A
	/// machine has at most one, and an identity belongs to at most one
	/// machine, so resolving either from the other is unambiguous.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub device_id: Option<Uuid>,
	/// Whether this machine is hosted in the cloud, if known.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cloud: Option<bool>,
	/// Where this machine is, if known.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub geolocation: Option<GeoPoint>,
	/// How long this machine may go without reporting before it is considered
	/// unreachable. Only enforced while `is_monitored`; the value is kept
	/// while unmonitored so turning monitoring back on does not lose it.
	#[schema(value_type = i64)]
	pub alert_when_down_for: PgDuration,
	/// Whether this machine is actively monitored. Switching it off quiets
	/// the machine's own checks and does not touch the applications on it —
	/// a box excused from monitoring says nothing about its workloads.
	pub is_monitored: bool,
	/// Free-form operator notes about this machine.
	#[serde(default)]
	pub notes: String,
	/// Key/value tags for this machine. A check filed against a machine is
	/// graded by policy against these rather than against any application's.
	/// An application's type is not among them, not being a property of a box.
	#[serde(default)]
	pub tags: TagMap,
	/// When set, the machine is archived: out of the live fleet, with its
	/// record and history retained. Archiving a machine archives the
	/// applications on it.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub deleted_at: Option<Timestamp>,
	/// When an identity completed enrolment for this machine. While `None`,
	/// the machine is awaiting its first check-in.
	///
	/// Also the anchor a backup deadline counts from, which is why it belongs
	/// to the machine: anchoring on an application's registration would
	/// restart a box's backup clock every time a workload was added to it.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub registered_at: Option<Timestamp>,
	#[serde(skip)]
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[serde(skip)]
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

/// The fields an operator supplies when creating a machine. Everything else
/// either has a default or arrives by enrolment and reporting.
#[derive(Debug, Clone, Default, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::machines)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewMachine {
	pub name: Option<String>,
	pub group_id: Option<Uuid>,
	pub cloud: Option<bool>,
	pub geolocation: Option<GeoPoint>,
}

impl Machine {
	/// Create a machine. An operator supplies the group; enrolment and
	/// reporting fill in the rest.
	pub async fn create(db: &mut AsyncPgConnection, new: NewMachine) -> Result<Self> {
		diesel::insert_into(crate::schema::machines::table)
			.values(new)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, machine_id: Uuid) -> Result<Self> {
		use crate::schema::machines::dsl;
		dsl::machines
			.select(Self::as_select())
			.filter(dsl::id.eq(machine_id))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_ids(db: &mut AsyncPgConnection, ids: &[Uuid]) -> Result<Vec<Self>> {
		use crate::schema::machines::dsl;
		if ids.is_empty() {
			return Ok(Vec::new());
		}
		dsl::machines
			.select(Self::as_select())
			.filter(dsl::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The machine an identity speaks for, if it speaks for one at all.
	///
	/// An identity that authenticates something other than a machine — an
	/// operator's credential, a relay — belongs to no machine, so this is the
	/// resolution step a machine-gated route takes and an admin-gated one
	/// never reaches.
	// spec: FLT#identities
	pub async fn get_by_device_id(
		db: &mut AsyncPgConnection,
		device: Uuid,
	) -> Result<Option<Self>> {
		use crate::schema::machines::dsl;
		dsl::machines
			.select(Self::as_select())
			.filter(dsl::device_id.eq(device))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Every machine still in the live fleet.
	pub async fn list_live(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::machines::dsl;
		dsl::machines
			.select(Self::as_select())
			.filter(dsl::deleted_at.is_null())
			.order(dsl::name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The live machines in a group.
	pub async fn list_for_group(db: &mut AsyncPgConnection, group: Uuid) -> Result<Vec<Self>> {
		use crate::schema::machines::dsl;
		dsl::machines
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group))
			.filter(dsl::deleted_at.is_null())
			.order(dsl::name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The applications running on this machine.
	pub async fn applications(
		&self,
		db: &mut AsyncPgConnection,
	) -> Result<Vec<crate::applications::Application>> {
		use crate::schema::applications::dsl;
		dsl::applications
			.select(crate::applications::Application::as_select())
			.filter(dsl::machine_id.eq(self.id))
			.filter(dsl::deleted_at.is_null())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// This machine's tags over its group's, so a check filed against a
	/// machine is graded by policy against the tags of its own target rather
	/// than against some application that happens to run on it.
	///
	/// An application's type is not among them: it is not a property of a box.
	// spec: FLT#what-each-carries
	pub async fn tags_merged_with_group(&self, db: &mut AsyncPgConnection) -> Result<TagMap> {
		let Some(gid) = self.group_id else {
			return Ok(self.tags.clone());
		};
		let group = crate::server_groups::ServerGroup::get_by_id(db, gid).await?;
		Ok(self.tags.merged_with(&group.tags))
	}

	/// Archive a machine and, with it, the applications on it — a box going
	/// away takes its workloads with it. Archival is not deletion: the records
	/// and their history remain.
	// spec: FLT#archival
	pub async fn archive(db: &mut AsyncPgConnection, machine_id: Uuid) -> Result<()> {
		let now = jiff_diesel::Timestamp::from(Timestamp::now());
		diesel::update(crate::schema::machines::table)
			.filter(crate::schema::machines::id.eq(machine_id))
			.filter(crate::schema::machines::deleted_at.is_null())
			.set(crate::schema::machines::deleted_at.eq(Some(now)))
			.execute(db)
			.await?;
		diesel::update(crate::schema::applications::table)
			.filter(crate::schema::applications::machine_id.eq(machine_id))
			.filter(crate::schema::applications::deleted_at.is_null())
			.set(crate::schema::applications::deleted_at.eq(Some(now)))
			.execute(db)
			.await?;
		Ok(())
	}
}
