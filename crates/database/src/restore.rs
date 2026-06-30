//! Managed restore replicas (RST): the control-plane state for driving an
//! external restore consumer. Operators declare which replicas should exist
//! ([`RestoreReplica`]); consumers register the intents they can satisfy
//! ([`RestoreConsumerCapability`]). The worklist expansion, credential issuance,
//! and restore-health ingest live in the public-server and `jobs` components.

use commons_errors::{AppError, Result};
use commons_types::backup::{BackupType, RestoreIntent};
use diesel::{
	prelude::*,
	result::{DatabaseErrorKind, Error as DieselError},
};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;

use crate::pg_duration::PgDuration;

/// An operator-declared replica: a consumer should keep a replica of a
/// `(group, [server | all servers], type)` for a given intent. The declaration
/// is both the work item (it expands into worklist entries) and the
/// authorization (it grants the consumer read access to that `(group, type)`).
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::restore_replicas)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct RestoreReplica {
	pub id: Uuid,
	pub consumer_device_id: Uuid,
	pub group_id: Uuid,
	/// `None` = all current servers in the group, expanded at worklist time.
	pub server_id: Option<Uuid>,
	#[diesel(column_name = type_)]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	pub name: String,
	/// Max age of the restored snapshot before the replica is overdue, in
	/// whole seconds; `None` = always track the latest snapshot.
	#[schema(value_type = Option<i64>)]
	pub freshness: Option<PgDuration>,
	pub enabled: bool,
	pub created_by: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::restore_replicas)]
pub struct NewRestoreReplica {
	pub consumer_device_id: Uuid,
	pub group_id: Uuid,
	pub server_id: Option<Uuid>,
	#[diesel(column_name = type_)]
	pub r#type: BackupType,
	pub intent: RestoreIntent,
	pub name: String,
	pub freshness: Option<PgDuration>,
	pub created_by: Option<String>,
}

impl RestoreReplica {
	/// Create a declaration. A duplicate `(consumer, group, type, intent,
	/// server)` scope maps to `409`.
	pub async fn create(db: &mut AsyncPgConnection, new: NewRestoreReplica) -> Result<Self> {
		use crate::schema::restore_replicas::dsl;
		match diesel::insert_into(dsl::restore_replicas)
			.values(new)
			.returning(Self::as_select())
			.get_result(db)
			.await
		{
			Ok(row) => Ok(row),
			Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => Err(
				AppError::Conflict("a matching restore replica is already declared".into()),
			),
			Err(e) => Err(AppError::from(e)),
		}
	}

	/// Every declaration, newest first — the operator overview.
	pub async fn list_all(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::restore_replicas::dsl;
		dsl::restore_replicas
			.select(Self::as_select())
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Declarations scoped to a group.
	pub async fn list_for_group(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::restore_replicas::dsl;
		dsl::restore_replicas
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Enabled declarations for a consumer — the basis of its worklist (before
	/// per-server expansion and capability filtering).
	pub async fn list_enabled_for_consumer(
		db: &mut AsyncPgConnection,
		consumer_device_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::restore_replicas::dsl;
		dsl::restore_replicas
			.select(Self::as_select())
			.filter(dsl::consumer_device_id.eq(consumer_device_id))
			.filter(dsl::enabled.eq(true))
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		use crate::schema::restore_replicas::dsl;
		dsl::restore_replicas
			.select(Self::as_select())
			.filter(dsl::id.eq(id))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?
			.ok_or(AppError::DatabaseQuery(DieselError::NotFound))
	}

	/// Edit the non-structural fields. Scope fields (consumer, group, server,
	/// type, intent) are immutable — change them by deleting and recreating.
	pub async fn update(
		db: &mut AsyncPgConnection,
		id: Uuid,
		name: &str,
		freshness: Option<PgDuration>,
		enabled: bool,
	) -> Result<Self> {
		use crate::schema::restore_replicas::dsl;
		diesel::update(dsl::restore_replicas.filter(dsl::id.eq(id)))
			.set((
				dsl::name.eq(name),
				dsl::freshness.eq(freshness),
				dsl::enabled.eq(enabled),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.optional()
			.map_err(AppError::from)?
			.ok_or(AppError::DatabaseQuery(DieselError::NotFound))
	}

	pub async fn delete(db: &mut AsyncPgConnection, id: Uuid) -> Result<()> {
		use crate::schema::restore_replicas::dsl;
		let n = diesel::delete(dsl::restore_replicas.filter(dsl::id.eq(id)))
			.execute(db)
			.await?;
		if n == 0 {
			return Err(AppError::DatabaseQuery(DieselError::NotFound));
		}
		Ok(())
	}

	/// Whether an enabled declaration covers `(consumer, group, type)` — the
	/// authorization check for issuing restore credentials. A server-scoped or
	/// a group-wide declaration both satisfy it.
	pub async fn authorizes(
		db: &mut AsyncPgConnection,
		consumer_device_id: Uuid,
		group_id: Uuid,
		r#type: &BackupType,
	) -> Result<bool> {
		use crate::schema::restore_replicas::dsl;
		let n: i64 = dsl::restore_replicas
			.filter(dsl::consumer_device_id.eq(consumer_device_id))
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::type_.eq(r#type.as_str()))
			.filter(dsl::enabled.eq(true))
			.count()
			.get_result(db)
			.await?;
		Ok(n > 0)
	}
}

/// One intent a consumer can satisfy. The full set is registered by the
/// consumer on start and whenever it changes; Canopy dispatches only matching
/// worklist entries and constrains the declaration UX to this set.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::restore_consumer_capabilities)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct RestoreConsumerCapability {
	pub consumer_device_id: Uuid,
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub registered_at: Timestamp,
}

impl RestoreConsumerCapability {
	/// Replace a consumer's capability set with `intents`. Implemented as
	/// insert-then-prune (not a transaction) so there is never a window where
	/// a still-valid intent is absent: new intents are inserted first, then any
	/// no longer present are removed.
	pub async fn register(
		db: &mut AsyncPgConnection,
		consumer_device_id: Uuid,
		intents: &[RestoreIntent],
	) -> Result<()> {
		use crate::schema::restore_consumer_capabilities::dsl;

		let strings: Vec<String> = intents.iter().map(|i| i.as_str().to_owned()).collect();

		let rows: Vec<_> = intents
			.iter()
			.map(|i| {
				(
					dsl::consumer_device_id.eq(consumer_device_id),
					dsl::intent.eq(i.as_str().to_owned()),
				)
			})
			.collect();
		if !rows.is_empty() {
			diesel::insert_into(dsl::restore_consumer_capabilities)
				.values(rows)
				.on_conflict((dsl::consumer_device_id, dsl::intent))
				.do_nothing()
				.execute(db)
				.await?;
		}

		diesel::delete(
			dsl::restore_consumer_capabilities
				.filter(dsl::consumer_device_id.eq(consumer_device_id))
				.filter(dsl::intent.ne_all(strings)),
		)
		.execute(db)
		.await?;
		Ok(())
	}

	/// The intents a consumer currently supports.
	pub async fn list_for_consumer(
		db: &mut AsyncPgConnection,
		consumer_device_id: Uuid,
	) -> Result<Vec<RestoreIntent>> {
		use crate::schema::restore_consumer_capabilities::dsl;
		let rows: Vec<String> = dsl::restore_consumer_capabilities
			.filter(dsl::consumer_device_id.eq(consumer_device_id))
			.select(dsl::intent)
			.order(dsl::intent.asc())
			.load(db)
			.await?;
		Ok(rows.into_iter().map(RestoreIntent::from).collect())
	}
}
