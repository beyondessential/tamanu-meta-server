//! Managed restore replicas (RST): the control-plane state for driving an
//! external restore consumer. Operators declare which replicas should exist
//! ([`RestoreReplica`]); consumers register the intents they can satisfy
//! ([`RestoreConsumerCapability`]). The worklist expansion, credential issuance,
//! and restore-health ingest live in the public-server and `jobs` components.

use std::collections::HashMap;

use commons_errors::{AppError, Result};
use commons_types::backup::{BackupType, RestoreIntent, RunOutcome};
use commons_types::issue::Severity;
use diesel::{
	prelude::*,
	result::{DatabaseErrorKind, Error as DieselError},
};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;

use crate::backup::alerts::raise_group_event;
use crate::backup::refs;
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
	/// Max time since the last healthy restore before the replica is overdue
	/// — the consumer's *restore* cadence (download + restore + any hold), not
	/// the backup interval. In whole seconds; `None` = no overdue bound.
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

/// The stable group-level alert ref for one replica's restore-health. Per
/// `(server, type, intent)` so each replica recovers independently (a healthy
/// `verify` must not clear a failing `disaster-recovery` on the same server).
fn restore_verification_ref(
	server_id: Uuid,
	r#type: &BackupType,
	intent: &RestoreIntent,
) -> String {
	format!(
		"{}:{}:{}:{}",
		refs::RESTORE_VERIFICATION,
		server_id,
		r#type,
		intent
	)
}

/// A restore-health report: one row per report a consumer sends about a
/// replica — proof a snapshot actually restored into a healthy database, the
/// strongest backup-health signal. `snapshot_id` joins back to the
/// produced/persisted record for that snapshot.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_restore_checks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRestoreCheck {
	pub id: i64,
	pub replica_id: Option<Uuid>,
	pub consumer_device_id: Uuid,
	pub group_id: Uuid,
	pub server_id: Option<Uuid>,
	#[diesel(column_name = type_)]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	pub snapshot_id: Option<String>,
	#[schema(value_type = String)]
	pub outcome: RunOutcome,
	pub error: Option<String>,
	pub replica_healthy: bool,
	pub postgres_version: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub observed_at: Timestamp,
	pub s3_sent_raw_bytes: Option<i64>,
	pub s3_sent_payload_bytes: Option<i64>,
	pub s3_received_raw_bytes: Option<i64>,
	pub s3_received_payload_bytes: Option<i64>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub reported_at: Timestamp,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::backup_restore_checks)]
pub struct NewBackupRestoreCheck {
	pub replica_id: Option<Uuid>,
	pub consumer_device_id: Uuid,
	pub group_id: Uuid,
	pub server_id: Option<Uuid>,
	#[diesel(column_name = type_)]
	pub r#type: BackupType,
	pub intent: RestoreIntent,
	pub snapshot_id: Option<String>,
	pub outcome: RunOutcome,
	pub error: Option<String>,
	pub replica_healthy: bool,
	pub postgres_version: Option<String>,
	#[diesel(serialize_as = jiff_diesel::Timestamp)]
	pub observed_at: Timestamp,
	pub s3_sent_raw_bytes: Option<i64>,
	pub s3_sent_payload_bytes: Option<i64>,
	pub s3_received_raw_bytes: Option<i64>,
	pub s3_received_payload_bytes: Option<i64>,
}

impl BackupRestoreCheck {
	/// Record a restore-health report and raise or recover its group-level
	/// alert. A success-and-healthy report recovers the replica's
	/// `restore-verification` issue; any other outcome raises it (`Error`,
	/// group-level, pages regardless of `is_monitored`).
	pub async fn record_report(
		db: &mut AsyncPgConnection,
		new: NewBackupRestoreCheck,
	) -> Result<()> {
		use crate::schema::backup_restore_checks::dsl;

		let healthy = new.outcome == RunOutcome::Success && new.replica_healthy;
		let group_id = new.group_id;
		let server_id = new.server_id;
		let r#type = new.r#type.clone();
		let intent = new.intent.clone();
		let error = new.error.clone();
		let snapshot_id = new.snapshot_id.clone();

		diesel::insert_into(dsl::backup_restore_checks)
			.values(new)
			.execute(db)
			.await?;

		// Restore-health is attributed per server; a report without one is
		// recorded but raises no group-level incident.
		if let Some(sid) = server_id {
			let r#ref = restore_verification_ref(sid, &r#type, &intent);
			if healthy {
				raise_group_event(
					db,
					group_id,
					&r#ref,
					Severity::Info,
					None,
					&format!("Restore verification healthy: {type} / {intent} for server {sid}"),
					false,
				)
				.await?;
			} else {
				let detail =
					error.unwrap_or_else(|| "restored database did not come up healthy".into());
				let snap = snapshot_id
					.map(|s| format!(" (snapshot {s})"))
					.unwrap_or_default();
				raise_group_event(
					db,
					group_id,
					&r#ref,
					Severity::Error,
					Some("restore verification failed"),
					&format!(
						"Restore verification failed: {type} / {intent} for server {sid}{snap}: {detail}"
					),
					true,
				)
				.await?;
			}
		}
		Ok(())
	}

	/// Recent reports for a group, newest first — the operator restore-health
	/// view.
	pub async fn list_recent_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_restore_checks::dsl;
		dsl::backup_restore_checks
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::observed_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Recent reports across all groups, newest first.
	pub async fn list_recent(db: &mut AsyncPgConnection, limit: i64) -> Result<Vec<Self>> {
		use crate::schema::backup_restore_checks::dsl;
		dsl::backup_restore_checks
			.select(Self::as_select())
			.order(dsl::observed_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Latest *healthy* report timestamp per `(server, type, intent)` in a
	/// group — the freshness anchor the overdue sweep compares against.
	pub async fn latest_healthy_by_key_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<HashMap<(Uuid, BackupType, RestoreIntent), Timestamp>> {
		use crate::schema::backup_restore_checks::dsl;
		let rows: Vec<Self> = dsl::backup_restore_checks
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::server_id.is_not_null())
			.filter(dsl::outcome.eq(RunOutcome::Success))
			.filter(dsl::replica_healthy.eq(true))
			.distinct_on((dsl::server_id, dsl::type_, dsl::intent))
			.order_by((
				dsl::server_id,
				dsl::type_,
				dsl::intent,
				dsl::observed_at.desc(),
			))
			.load(db)
			.await?;
		Ok(rows
			.into_iter()
			.filter_map(|r| {
				r.server_id
					.map(|sid| ((sid, r.r#type.clone(), r.intent.clone()), r.observed_at))
			})
			.collect())
	}
}

/// Overdue restore-verification sweep: for every enabled declaration with a
/// freshness bound (whose intent the consumer still supports), raise the
/// `restore-verification` alert for any concrete `(server, type, intent)` whose
/// last healthy report is older than the bound or never happened. Recovery is
/// driven by the next healthy report ([`BackupRestoreCheck::record_report`]),
/// so this only raises. Returns the number of overdue alerts filed.
pub async fn sweep_overdue(db: &mut AsyncPgConnection) -> Result<usize> {
	use crate::schema::restore_replicas::dsl;
	let now = Timestamp::now();

	let declarations: Vec<RestoreReplica> = dsl::restore_replicas
		.select(RestoreReplica::as_select())
		.filter(dsl::enabled.eq(true))
		.filter(dsl::freshness.is_not_null())
		.load(db)
		.await?;

	let mut capability_cache: HashMap<Uuid, std::collections::HashSet<RestoreIntent>> =
		HashMap::new();
	let mut healthy_cache: HashMap<Uuid, HashMap<(Uuid, BackupType, RestoreIntent), Timestamp>> =
		HashMap::new();
	let mut filed = 0usize;

	for d in declarations {
		let Some(freshness) = d.freshness else {
			continue;
		};

		// Skip declarations the consumer can't satisfy — those are gaps, not
		// restore-health incidents.
		if !capability_cache.contains_key(&d.consumer_device_id) {
			let set = RestoreConsumerCapability::list_for_consumer(db, d.consumer_device_id)
				.await?
				.into_iter()
				.collect();
			capability_cache.insert(d.consumer_device_id, set);
		}
		if !capability_cache[&d.consumer_device_id].contains(&d.intent) {
			continue;
		}

		let servers = match d.server_id {
			Some(sid) => {
				let s = crate::servers::Server::get_by_id(db, sid).await.ok();
				match s {
					Some(s) if s.group_id == Some(d.group_id) && s.deleted_at.is_none() => {
						vec![sid]
					}
					_ => vec![],
				}
			}
			None => crate::servers::Server::list_live_in_group(db, d.group_id)
				.await?
				.into_iter()
				.map(|s| s.id)
				.collect(),
		};

		if !healthy_cache.contains_key(&d.group_id) {
			let map = BackupRestoreCheck::latest_healthy_by_key_for_group(db, d.group_id).await?;
			healthy_cache.insert(d.group_id, map);
		}
		let healthy = healthy_cache[&d.group_id].clone();

		for sid in servers {
			let key = (sid, d.r#type.clone(), d.intent.clone());
			let overdue = match healthy.get(&key) {
				Some(last) => now.duration_since(*last) > freshness.0,
				None => true,
			};
			if !overdue {
				continue;
			}
			let r#ref = restore_verification_ref(sid, &d.r#type, &d.intent);
			raise_group_event(
				db,
				d.group_id,
				&r#ref,
				Severity::Error,
				Some("restore verification overdue"),
				&format!(
					"No healthy restore verification within the freshness window: {} / {} for server {sid}",
					d.r#type, d.intent
				),
				true,
			)
			.await?;
			filed += 1;
		}
	}

	Ok(filed)
}
