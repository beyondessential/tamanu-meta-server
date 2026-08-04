//! Managed restore replicas (RST): the control-plane state for driving an
//! external restore consumer. Operators declare which replicas should exist
//! ([`RestoreReplica`]); consumers register the intents they can satisfy
//! ([`RestoreConsumerCapability`]). The worklist expansion, credential issuance,
//! and restore-health ingest live in the public-server and `jobs` components.

use std::collections::{HashMap, HashSet, hash_map::Entry};

use commons_errors::{AppError, Result};
use commons_types::backup::{
	BackupType, IntentDescriptor, RedactionOutcome, RestoreIntent, RunOutcome, semantics,
};
use commons_types::status::CheckResult;
use diesel::{
	prelude::*,
	result::{DatabaseErrorKind, Error as DieselError},
};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;

use crate::backup::refs;
use crate::backups::BackupRun;
use crate::issues::{
	CheckInstance, GradedInstance, InstancedCheckFiling, Scope, file_check_instances,
};
use crate::pg_duration::PgDuration;

/// An operator's declaration that a restore consumer should maintain a
/// restorable replica for a server (or every server in a group) and backup
/// type, satisfying a given restore intent. This both queues the work for
/// the consumer and grants that consumer access to the matching backups.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::restore_replicas)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct RestoreReplica {
	/// Unique identifier for this declaration.
	pub id: Uuid,
	/// The device (restore consumer) responsible for maintaining this
	/// replica.
	pub consumer_device_id: Uuid,
	/// The server group this declaration applies to.
	pub group_id: Uuid,
	/// The specific server this declaration covers. `None` means every
	/// server currently in the group.
	pub server_id: Option<Uuid>,
	/// The backup type this declaration covers (e.g. a database snapshot
	/// type). Any type name the consumer advertises support for is
	/// accepted.
	#[diesel(column_name = type_)]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// The restore intent this declaration should satisfy — what kind of
	/// restore behaviour the consumer should perform against the backups
	/// (for example, a periodic verification restore vs. one held ready for
	/// disaster recovery). Any intent name the consumer advertises support
	/// for is accepted.
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// An operator-assigned label for this declaration, shown in the UI.
	pub name: String,
	/// How long, in seconds, this replica may go without a healthy restore
	/// report before it's considered overdue and an alert is raised. For an
	/// intent that only needs to verify the latest snapshot once, this
	/// instead bounds how long that snapshot may go unverified. `None`
	/// means no overdue bound is enforced.
	#[schema(value_type = Option<i64>)]
	pub overdue_after: Option<PgDuration>,
	/// Operator-supplied parameter values for this declaration, keyed by
	/// parameter name, passed through to the consumer. Only the values the
	/// operator explicitly set are included.
	#[schema(value_type = Object)]
	pub params: serde_json::Value,
	/// Whether this replica is served de-identified. Canopy resolves the
	/// masking manifest itself from the server's product, so this flag is
	/// the whole of the operator's say in it, and it answers on its own
	/// whether a replica that came up unmasked is a finding.
	pub redacts: bool,
	/// Whether this declaration is currently active. When disabled, it
	/// produces no work and grants no access, but is kept for reference.
	pub enabled: bool,
	/// The operator who created this declaration. `None` if not recorded.
	pub created_by: Option<String>,
	/// When this declaration was created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// When this declaration was last modified.
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
	pub overdue_after: Option<PgDuration>,
	pub params: serde_json::Value,
	pub redacts: bool,
	pub created_by: Option<String>,
}

/// The full new state of a declaration, including its scope. Every field is
/// always set (there is no partial-update shorthand); `server_id: None` means
/// "every server in the group".
#[derive(Debug, Clone)]
pub struct RestoreReplicaUpdate {
	pub consumer_device_id: Uuid,
	pub group_id: Uuid,
	pub server_id: Option<Uuid>,
	pub r#type: BackupType,
	pub intent: RestoreIntent,
	pub name: String,
	pub overdue_after: Option<PgDuration>,
	pub params: serde_json::Value,
	pub redacts: bool,
	pub enabled: bool,
}

/// Normalise an operator-supplied declaration name: surrounding whitespace is
/// insignificant, and a name that is blank once trimmed is rejected. Names are
/// unique per consumer, so leaving the whitespace on would let a near-identical
/// name slip past the index.
fn normalize_name(name: String) -> Result<String> {
	let trimmed = name.trim();
	if trimmed.is_empty() {
		return Err(AppError::BadRequest(
			"restore replica name cannot be empty".into(),
		));
	}
	Ok(trimmed.to_owned())
}

/// Turn a unique violation into a `409` that names the collision actually hit —
/// a declaration's scope and its name are separately unique, and the operator
/// needs to know which one to change.
fn unique_violation(info: &dyn diesel::result::DatabaseErrorInformation) -> AppError {
	match info.constraint_name() {
		Some("restore_replicas_consumer_name") => {
			AppError::Conflict("this consumer already has a restore replica with that name".into())
		}
		_ => AppError::Conflict("a matching restore replica is already declared".into()),
	}
}

impl RestoreReplica {
	/// Create a declaration. A duplicate `(consumer, group, type, intent,
	/// server)` scope, or a name already used by another of the consumer's
	/// declarations, maps to `409`.
	pub async fn create(db: &mut AsyncPgConnection, new: NewRestoreReplica) -> Result<Self> {
		use crate::schema::restore_replicas::dsl;
		let new = NewRestoreReplica {
			name: normalize_name(new.name)?,
			..new
		};
		match diesel::insert_into(dsl::restore_replicas)
			.values(new)
			.returning(Self::as_select())
			.get_result(db)
			.await
		{
			Ok(row) => Ok(row),
			Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info)) => {
				Err(unique_violation(&*info))
			}
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

	/// Edit every field of a declaration, including its scope. A scope change
	/// that collides with another declaration's `(consumer, group, type,
	/// intent, server)`, or a name already used by another of the consumer's
	/// declarations, maps to `409`, same as [`Self::create`]. If the scope
	/// (group, server, or type/intent) moves, or the declaration is disabled,
	/// any active restore-verification alert keyed to the *old* scope is
	/// recovered — the overdue sweep only walks current *enabled* declarations,
	/// so a stale key would otherwise never clear.
	pub async fn update(
		db: &mut AsyncPgConnection,
		id: Uuid,
		update: RestoreReplicaUpdate,
	) -> Result<Self> {
		use crate::schema::restore_replicas::dsl;

		let name = normalize_name(update.name)?;

		let result = match diesel::update(dsl::restore_replicas.filter(dsl::id.eq(id)))
			.set((
				dsl::consumer_device_id.eq(update.consumer_device_id),
				dsl::group_id.eq(update.group_id),
				dsl::server_id.eq(update.server_id),
				dsl::type_.eq(update.r#type),
				dsl::intent.eq(update.intent),
				dsl::name.eq(name),
				dsl::overdue_after.eq(update.overdue_after),
				dsl::params.eq(update.params),
				dsl::redacts.eq(update.redacts),
				dsl::enabled.eq(update.enabled),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
		{
			Ok(row) => row,
			Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info)) => {
				return Err(unique_violation(&*info));
			}
			Err(DieselError::NotFound) => {
				return Err(AppError::DatabaseQuery(DieselError::NotFound));
			}
			Err(e) => return Err(AppError::from(e)),
		};

		// Nothing to clear by hand when the scope moves, the declaration is
		// disabled, or it is deleted: `sweep_overdue` rebuilds each server's
		// restore checks from its live declarations every pass, so a
		// declaration that stops covering a server simply stops being one of
		// its instances and the check re-files without it. That is what
		// recomputing buys over accumulating.
		Ok(result)
	}

	/// Delete a declaration. Its restore checks need no explicit recovery: the
	/// next [`sweep_overdue`] pass rebuilds each server's checks from its live
	/// declarations, so a deleted one stops being an instance and the check
	/// re-files without it.
	///
	/// Restore-health reports the declaration collected are retained: the FK
	/// from `backup_restore_checks` is `ON DELETE SET NULL`, so each report
	/// survives with its group, server, type, and intent, no longer attached to
	/// a declaration.
	pub async fn delete(db: &mut AsyncPgConnection, id: Uuid) -> Result<()> {
		db.transaction::<_, AppError, _>(async |conn| {
			use crate::schema::restore_replicas::dsl;
			let n = diesel::delete(dsl::restore_replicas.filter(dsl::id.eq(id)))
				.execute(conn)
				.await?;
			if n == 0 {
				return Err(AppError::DatabaseQuery(DieselError::NotFound));
			}
			// See `update`: the next sweep rebuilds the server's restore
			// checks from its live declarations, so a deleted one drops out on
			// its own.
			Ok(())
		})
		.await
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

/// A consumer's advertised capability row: one intent it can satisfy, with the
/// description, semantics, and parameter schema it advertises. Registered by the
/// consumer on start and whenever it changes; Canopy dispatches only matching
/// worklist entries, acts on the semantics it recognises, and constrains the
/// declaration UX to this set.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = crate::schema::restore_consumer_capabilities)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct CapabilityRow {
	#[allow(dead_code)]
	consumer_device_id: Uuid,
	intent: RestoreIntent,
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	#[allow(dead_code)]
	registered_at: Timestamp,
	description: Option<String>,
	semantics: serde_json::Value,
	params: serde_json::Value,
}

impl CapabilityRow {
	fn into_descriptor(self) -> IntentDescriptor {
		IntentDescriptor {
			intent: self.intent,
			description: self.description,
			semantics: serde_json::from_value(self.semantics).unwrap_or_default(),
			params: serde_json::from_value(self.params).unwrap_or_default(),
		}
	}
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::restore_consumer_capabilities)]
struct NewCapability {
	consumer_device_id: Uuid,
	intent: RestoreIntent,
	description: Option<String>,
	semantics: serde_json::Value,
	params: serde_json::Value,
}

/// Registration and lookup of a consumer's advertised capabilities.
pub struct RestoreConsumerCapability;

impl RestoreConsumerCapability {
	/// Replace a consumer's advertised set with `descriptors`. Implemented as
	/// upsert-then-prune (not a transaction) so there is never a window where a
	/// still-valid intent is absent: advertised intents are upserted first, then
	/// any no longer present are removed.
	pub async fn register(
		db: &mut AsyncPgConnection,
		consumer_device_id: Uuid,
		descriptors: &[IntentDescriptor],
	) -> Result<()> {
		use crate::schema::restore_consumer_capabilities::dsl;
		use diesel::upsert::excluded;

		let names: Vec<String> = descriptors
			.iter()
			.map(|d| d.intent.as_str().to_owned())
			.collect();

		let rows: Vec<NewCapability> = descriptors
			.iter()
			.map(|d| NewCapability {
				consumer_device_id,
				intent: d.intent.clone(),
				description: d.description.clone(),
				semantics: serde_json::to_value(&d.semantics).expect("semantics serialize"),
				params: serde_json::to_value(&d.params).expect("params serialize"),
			})
			.collect();

		if !rows.is_empty() {
			diesel::insert_into(dsl::restore_consumer_capabilities)
				.values(rows)
				.on_conflict((dsl::consumer_device_id, dsl::intent))
				.do_update()
				.set((
					dsl::description.eq(excluded(dsl::description)),
					dsl::semantics.eq(excluded(dsl::semantics)),
					dsl::params.eq(excluded(dsl::params)),
				))
				.execute(db)
				.await?;
		}

		diesel::delete(
			dsl::restore_consumer_capabilities
				.filter(dsl::consumer_device_id.eq(consumer_device_id))
				.filter(dsl::intent.ne_all(names)),
		)
		.execute(db)
		.await?;
		Ok(())
	}

	/// The descriptors a consumer currently advertises, ordered by intent.
	pub async fn list_for_consumer(
		db: &mut AsyncPgConnection,
		consumer_device_id: Uuid,
	) -> Result<Vec<IntentDescriptor>> {
		use crate::schema::restore_consumer_capabilities::dsl;
		let rows: Vec<CapabilityRow> = dsl::restore_consumer_capabilities
			.filter(dsl::consumer_device_id.eq(consumer_device_id))
			.select(CapabilityRow::as_select())
			.order(dsl::intent.asc())
			.load(db)
			.await?;
		Ok(rows
			.into_iter()
			.map(CapabilityRow::into_descriptor)
			.collect())
	}
}

/// Why a server a redacting declaration covers can't be redacted.
///
/// Each of these withholds the server's worklist entry: a replica that
/// cannot be redacted is not restored at all, since an unredacted replica
/// standing in for a redacted one is worse than no replica.
// spec: RST#the-masking-manifest
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactionGapReason {
	/// The server's product publishes no masking manifests, so the replica is
	/// withheld from the worklist rather than restored unmasked.
	ProductHasNoManifest,
	/// The product publishes them, but not for the version this server
	/// reports, so the consumer would fetch a URL that 404s. The replica is
	/// still dispatched — the consumer resolves the manifest against the
	/// version in the data it restored, which may not be this one, and holds
	/// the switchover if it can't.
	VersionHasNoManifest,
}

/// Whether a server can be redacted, and why not when it can't.
///
/// Corroborates the product's manifest template against the artefacts the
/// version actually published: a template that resolves to nothing is
/// caught here, at declaration time, rather than when a restore fails.
// spec: RST#the-masking-manifest
pub async fn redaction_gap_for(
	db: &mut AsyncPgConnection,
	server: &crate::servers::Server,
) -> Result<Option<(RedactionGapReason, Option<String>)>> {
	let Some(manifest) = server.product.caps().redaction else {
		return Ok(Some((RedactionGapReason::ProductHasNoManifest, None)));
	};

	// The consumer resolves the manifest against the version in the data it
	// restored, not against what the server last reported, so a server Canopy
	// holds no version for isn't one that can't be redacted — it's one Canopy
	// can't corroborate, which is not a finding.
	let Some(reported) =
		crate::reported_detail::ReportedDetail::last_version(db, server.id).await?
	else {
		return Ok(None);
	};
	let shown = reported.0.to_string();

	// An unpublished version has no artefacts at all, which reads the same
	// way as a published one that didn't upload a manifest: either way the
	// URL the consumer would fetch isn't there.
	let Ok(version) = crate::versions::Version::get_by_version(db, reported).await else {
		return Ok(Some((
			RedactionGapReason::VersionHasNoManifest,
			Some(shown),
		)));
	};

	let published = crate::artifacts::Artifact::get_for_version(db, version.id)
		.await?
		.into_iter()
		.any(|a| a.artifact_type == manifest.artifact_type);

	Ok((!published).then_some((RedactionGapReason::VersionHasNoManifest, Some(shown))))
}

/// A restore-health report submitted by a restore consumer: proof (or
/// disproof) that a backup snapshot actually restores into a healthy
/// database — the strongest available signal of backup health. `snapshot_id`
/// identifies which snapshot was restored, if the report is snapshot-scoped.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_restore_checks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRestoreCheck {
	/// Unique identifier for this report.
	pub id: i64,
	/// The replica declaration this report is for, if it was made against
	/// a declared replica. `None` for reports not tied to a declaration.
	pub replica_id: Option<Uuid>,
	/// The device (restore consumer) that submitted this report.
	pub consumer_device_id: Uuid,
	/// The server group this report belongs to.
	pub group_id: Uuid,
	/// The server this report is about, if any.
	pub server_id: Option<Uuid>,
	/// The backup type this report covers.
	#[diesel(column_name = type_)]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// The restore intent this report was performed for.
	#[schema(value_type = String)]
	pub intent: RestoreIntent,
	/// The id of the snapshot that was restored, if the report is scoped to
	/// a specific snapshot.
	pub snapshot_id: Option<String>,
	/// Whether the restore attempt itself succeeded or failed.
	#[schema(value_type = String)]
	pub outcome: RunOutcome,
	/// Error message reported for a failed restore, if any.
	pub error: Option<String>,
	/// Whether the restored database came up and passed its health checks.
	/// A report only counts as fully healthy when `outcome` is success and
	/// this is also `true`.
	pub replica_healthy: bool,
	/// The Postgres version of the restored database, if reported.
	pub postgres_version: Option<String>,
	/// When the restore attempt this report describes actually took place.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub observed_at: Timestamp,
	/// Bytes sent to storage for this restore, counting the full HTTP
	/// request including signing/chunking overhead.
	pub s3_sent_raw_bytes: Option<i64>,
	/// Bytes sent to storage for this restore, counting only the decoded
	/// object data (excludes request/signing overhead).
	pub s3_sent_payload_bytes: Option<i64>,
	/// Bytes received from storage for this restore, counting the full HTTP
	/// response including framing overhead.
	pub s3_received_raw_bytes: Option<i64>,
	/// Bytes received from storage for this restore, counting only the
	/// decoded object data (excludes response framing overhead).
	pub s3_received_payload_bytes: Option<i64>,
	/// When this report was received.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub reported_at: Timestamp,
	/// Additional health data supplied by the consumer (for example,
	/// database cluster statistics or whether indexes needed repair).
	/// Passed through and displayed as-is. `None` if none was supplied.
	pub health_details: Option<serde_json::Value>,
	/// The run this report belongs to, when the consumer stamped its run-uuid on
	/// the restore-credentials request. Ties the report to its issuance exactly
	/// (for Canopy-measured duration). `None` for older consumers, which fall
	/// back to time-window matching.
	pub run_id: Option<Uuid>,
	/// How far the replica's masking manifest got. `None` for a replica that
	/// doesn't redact.
	pub redaction_outcome: Option<RedactionOutcome>,
	/// The version whose manifest was fetched, when the manifest URL named one.
	pub redaction_manifest_version: Option<String>,
	/// How many columns the manifest masked.
	pub redaction_columns_masked: Option<i64>,
	/// How many columns the manifest named but could not mask. Non-zero is
	/// what makes a redaction partial.
	pub redaction_columns_skipped: Option<i64>,
	/// Why the redaction failed, when it did. Distinct from `error`, which
	/// describes the restore: the restore can succeed and the redaction that
	/// follows it fail.
	pub redaction_error: Option<String>,
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
	pub health_details: Option<serde_json::Value>,
	pub run_id: Option<Uuid>,
	pub redaction_outcome: Option<RedactionOutcome>,
	pub redaction_manifest_version: Option<String>,
	pub redaction_columns_masked: Option<i64>,
	pub redaction_columns_skipped: Option<i64>,
	pub redaction_error: Option<String>,
}

impl BackupRestoreCheck {
	/// Record a restore-health report.
	///
	/// Recording is all this does: the checks the report feeds —
	/// `restore-verification` and `redaction` — are filed by [`sweep_overdue`],
	/// which is their sole filer. A replica's state is the worse of what its
	/// latest report said and whether it has gone overdue, and only the sweep
	/// holds every one of a server's replicas at once; this path holds one. Two
	/// filers on the same check would race and drift apart.
	///
	/// The sweep runs on the same minute cadence as the reachability tick, and
	/// these are non-paging warnings (see `BKJ#alerting`), so the delay between
	/// a report landing and its check reflecting it does not matter.
	pub async fn record_report(
		db: &mut AsyncPgConnection,
		new: NewBackupRestoreCheck,
	) -> Result<i64> {
		use crate::schema::backup_restore_checks::dsl;
		diesel::insert_into(dsl::backup_restore_checks)
			.values(new)
			.returning(dsl::id)
			.get_result(db)
			.await
			.map_err(AppError::from)
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

	/// Recent reports for one replica, newest first.
	///
	/// Not [`Self::list_recent_for_group`] filtered afterwards: the limit has
	/// to bite the replica's own checks, or a low-frequency replica sharing a
	/// group with a chatty one has every one of its reports pushed out of the
	/// window and reads as never checked.
	pub async fn list_recent_for_replica(
		db: &mut AsyncPgConnection,
		replica_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_restore_checks::dsl;
		dsl::backup_restore_checks
			.select(Self::as_select())
			.filter(dsl::replica_id.eq(Some(replica_id)))
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

	/// The latest report per `(server, type, intent)` in a group, whatever its
	/// outcome — what the sweep needs to know each replica's *current* state,
	/// as opposed to when it was last healthy.
	///
	/// Carries the whole row so the redaction fields come along: a report says
	/// both whether the restore came up healthy and whether its masking
	/// applied, and those are two checks off one report.
	pub async fn latest_by_key_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<HashMap<(Uuid, BackupType, RestoreIntent), Self>> {
		use crate::schema::backup_restore_checks::dsl;
		let rows: Vec<Self> = dsl::backup_restore_checks
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::server_id.is_not_null())
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
					.map(|sid| ((sid, r.r#type.clone(), r.intent.clone()), r))
			})
			.collect())
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

	/// The snapshot id of the most recent *healthy* report per
	/// `(server, type, intent)` in a group — the anchor for `once` suppression
	/// (a snapshot is already verified when this equals the latest snapshot) and
	/// snapshot-driven overdue. Reports without a snapshot id are ignored.
	pub async fn latest_healthy_snapshot_by_key_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<HashMap<(Uuid, BackupType, RestoreIntent), String>> {
		use crate::schema::backup_restore_checks::dsl;
		let rows: Vec<Self> = dsl::backup_restore_checks
			.select(Self::as_select())
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::server_id.is_not_null())
			.filter(dsl::snapshot_id.is_not_null())
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
			.filter_map(|r| match (r.server_id, r.snapshot_id) {
				(Some(sid), Some(snap)) => Some(((sid, r.r#type.clone(), r.intent.clone()), snap)),
				_ => None,
			})
			.collect())
	}
}

/// Overdue restore-verification sweep: for every enabled declaration with an
/// overdue bound whose intent the consumer still advertises with the `check`
/// semantic, raise the `restore-verification` alert for any concrete
/// `(server, type, intent)` that is overdue. Overdue is measured per the intent's
/// semantics: a `once` intent is overdue when the latest snapshot has gone
/// unverified for longer than the bound; any other `check` intent is overdue
/// when it has no healthy report within the bound. Recovery is driven by the
/// next healthy report ([`BackupRestoreCheck::record_report`]), so this only
/// raises. Returns the number of overdue alerts filed.
pub async fn sweep_overdue(db: &mut AsyncPgConnection) -> Result<usize> {
	use crate::schema::restore_replicas::dsl;
	let now = Timestamp::now();

	// Every enabled declaration, not just the ones with an overdue bound: this
	// sweep is the sole filer of the restore checks, so a declaration without a
	// bound still needs its latest report's health reflected. A missing bound
	// means "never overdue", not "never checked".
	let declarations: Vec<RestoreReplica> = dsl::restore_replicas
		.select(RestoreReplica::as_select())
		.filter(dsl::enabled.eq(true))
		.load(db)
		.await?;

	// Per-consumer descriptors (to read semantics) and per-group anchors.
	let mut capability_cache: HashMap<Uuid, HashMap<RestoreIntent, IntentDescriptor>> =
		HashMap::new();
	let mut healthy_cache: HashMap<Uuid, HashMap<(Uuid, BackupType, RestoreIntent), Timestamp>> =
		HashMap::new();
	let mut verified_snapshot_cache: HashMap<
		Uuid,
		HashMap<(Uuid, BackupType, RestoreIntent), String>,
	> = HashMap::new();
	let mut latest_snapshot_cache: HashMap<Uuid, HashMap<(Uuid, BackupType), BackupRun>> =
		HashMap::new();
	let mut latest_report_cache: HashMap<
		Uuid,
		HashMap<(Uuid, BackupType, RestoreIntent), BackupRestoreCheck>,
	> = HashMap::new();

	// Instances accumulate per (server, check) and are filed once at the end:
	// one restore-verification check per server, with its replicas as
	// instances, rather than one check per (type, intent). See the Names
	// section of the CHK spec.
	let mut verification: HashMap<Uuid, Vec<CheckInstance>> = HashMap::new();
	let mut redaction: HashMap<Uuid, Vec<CheckInstance>> = HashMap::new();
	let mut migration: HashMap<Uuid, Vec<CheckInstance>> = HashMap::new();
	let mut server_order: Vec<Uuid> = Vec::new();
	let mut seen_servers: HashSet<Uuid> = HashSet::new();

	for d in declarations {
		// Skip declarations the consumer can't satisfy — those are gaps, not
		// restore-health findings. Only `check` intents are held to a bound.
		let capabilities = match capability_cache.entry(d.consumer_device_id) {
			Entry::Occupied(e) => e.into_mut(),
			Entry::Vacant(e) => e.insert(
				RestoreConsumerCapability::list_for_consumer(db, d.consumer_device_id)
					.await?
					.into_iter()
					.map(|desc| (desc.intent.clone(), desc))
					.collect(),
			),
		};
		let Some(descriptor) = capabilities.get(&d.intent) else {
			continue;
		};
		if !descriptor.has_semantic(semantics::CHECK) {
			continue;
		}
		let once = descriptor.has_semantic(semantics::ONCE);
		let migrates = descriptor.has_semantic(semantics::MIGRATE);

		let servers: Vec<crate::servers::Server> = match d.server_id {
			Some(sid) => {
				let s = crate::servers::Server::get_by_id(db, sid).await.ok();
				match s {
					Some(s) if s.group_id == Some(d.group_id) && s.deleted_at.is_none() => {
						vec![s]
					}
					_ => vec![],
				}
			}
			None => crate::servers::Server::list_live_in_group(db, d.group_id).await?,
		};

		if let Entry::Vacant(e) = healthy_cache.entry(d.group_id) {
			e.insert(BackupRestoreCheck::latest_healthy_by_key_for_group(db, d.group_id).await?);
			verified_snapshot_cache.insert(
				d.group_id,
				BackupRestoreCheck::latest_healthy_snapshot_by_key_for_group(db, d.group_id)
					.await?,
			);
			latest_snapshot_cache.insert(
				d.group_id,
				BackupRun::latest_success_by_server_type_for_group(db, d.group_id).await?,
			);
			latest_report_cache.insert(
				d.group_id,
				BackupRestoreCheck::latest_by_key_for_group(db, d.group_id).await?,
			);
		}

		for server in servers {
			let sid = server.id;
			if seen_servers.insert(sid) {
				server_order.push(sid);
			}
			let label = replica_label(&d);
			let key = (sid, d.r#type.clone(), d.intent.clone());
			let latest = latest_report_cache[&d.group_id].get(&key);

			if migrates {
				let instance = migration_instance(
					db,
					&d,
					&server,
					&label,
					&latest_snapshot_cache[&d.group_id],
					now,
					d.overdue_after,
				)
				.await?;
				if let Some(instance) = instance {
					migration.entry(sid).or_default().push(instance);
				}
				continue;
			}

			// Overdue is a property of the bound, so a declaration without one
			// is never overdue — but its latest report's health still counts.
			let overdue = match d.overdue_after {
				None => false,
				Some(overdue_after) if once => {
					// A `once` intent is overdue only when a snapshot exists to
					// verify, it is not the last one verified, and it has stood
					// past the bound.
					match latest_snapshot_cache[&d.group_id].get(&(sid, d.r#type.clone())) {
						None => false,
						Some(run) => {
							let verified = verified_snapshot_cache[&d.group_id].get(&key);
							let already = matches!(
								(verified, run.snapshot_id.as_ref()),
								(Some(v), Some(s)) if v == s
							);
							// Measured from the report, not `run.anchor()`: the
							// question is how long this snapshot has gone
							// unverified since it became available to verify,
							// which is when it landed — not how old the data
							// inside it is.
							!already && now.duration_since(run.reported_at) > overdue_after.0
						}
					}
				}
				Some(overdue_after) => match healthy_cache[&d.group_id].get(&key) {
					Some(last) => now.duration_since(*last) > overdue_after.0,
					None => true,
				},
			};

			// The replica's state is the worse of what its latest report said
			// and whether it has gone overdue. These used to be two writers
			// racing on one check name; now they are one instance's result.
			let reported_unhealthy =
				latest.is_some_and(|r| r.outcome != RunOutcome::Success || !r.replica_healthy);
			let observed = if reported_unhealthy || overdue {
				CheckResult::Failed
			} else {
				CheckResult::Passed
			};
			let why = if reported_unhealthy {
				latest
					.and_then(|r| r.error.clone())
					.unwrap_or_else(|| "restored database did not come up healthy".into())
			} else if overdue && once {
				"latest snapshot not verified within its overdue bound".into()
			} else if overdue {
				"no healthy restore verification within its overdue bound".into()
			} else {
				"healthy".into()
			};
			verification.entry(sid).or_default().push(CheckInstance {
				label: label.clone(),
				observed,
				detail: Some(serde_json::json!({
					"type": d.r#type.to_string(),
					"intent": d.intent.to_string(),
					"replica": d.name,
					"snapshot_id": latest.and_then(|r| r.snapshot_id.clone()),
					"overdue": overdue,
					"latest_snapshot_unverified": overdue && once,
					"why": why,
				})),
			});

			// Redaction is its own signal off the same report: a replica can
			// restore healthily and still come up with data that was meant to
			// be masked and isn't. Only a declaration that redacts has one.
			if d.redacts {
				if let Some(instance) = redaction_instance(&d, &label, latest) {
					redaction.entry(sid).or_default().push(instance);
				}
			}
		}
	}

	let mut filed = 0usize;
	for sid in server_order {
		let label = crate::backup::staleness::server_label(
			&crate::servers::Server::get_by_id(db, sid).await?,
		);
		if let Some(instances) = verification.remove(&sid) {
			filed += file_verification(db, sid, &label, instances).await?;
		}
		if let Some(instances) = redaction.remove(&sid) {
			filed += file_redaction(db, sid, &label, instances).await?;
		}
		if let Some(instances) = migration.remove(&sid) {
			filed += file_migration(db, sid, &label, instances).await?;
		}
	}

	Ok(filed)
}

/// How one replica is named in a check's message and detail: the operator's own
/// label for the declaration, qualified by what it covers, since a server can
/// have several replicas of the same type under different intents.
fn replica_label(d: &RestoreReplica) -> String {
	format!("{} ({} / {})", d.name, d.r#type, d.intent)
}

/// The redaction instance for one replica, from its latest report. `None` when
/// the replica has not reported a redaction outcome yet — nothing observed is
/// not the same as redacted.
fn redaction_instance(
	d: &RestoreReplica,
	label: &str,
	latest: Option<&BackupRestoreCheck>,
) -> Option<CheckInstance> {
	let latest = latest?;
	let outcome = latest.redaction_outcome?;
	let observed = match outcome {
		RedactionOutcome::Complete => CheckResult::Passed,
		RedactionOutcome::Partial | RedactionOutcome::Failed => CheckResult::Warning,
	};
	Some(CheckInstance {
		label: label.to_owned(),
		observed,
		detail: Some(serde_json::json!({
			"type": d.r#type.to_string(),
			"intent": d.intent.to_string(),
			"replica": d.name,
			"outcome": outcome.to_string(),
			"manifest_version": latest.redaction_manifest_version,
			"columns_masked": latest.redaction_columns_masked,
			"columns_skipped": latest.redaction_columns_skipped,
			"error": latest.redaction_error,
		})),
	})
}

async fn file_verification(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	label: &str,
	instances: Vec<CheckInstance>,
) -> Result<usize> {
	let total = instances.len();
	file_check_instances(
		db,
		InstancedCheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: Scope::Server(server_id),
			device_id: None,
			check: refs::RESTORE_VERIFICATION,
			title: Some("restore verification failed"),
			instances,
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: Some(refs::RESTORE_VERIFICATION_DOC),
		},
		&|degraded| match degraded {
			[] => format!("Every restore replica of {label} is verifying healthily"),
			[one] => format!(
				"Restore verification failed for {label}: {} — {}",
				one.label,
				instance_why(one),
			),
			many => format!(
				"Restore verification failed for {} of {label}'s {total} replicas: {}",
				many.len(),
				instance_labels(many),
			),
		},
	)
	.await?;
	Ok(1)
}

async fn file_redaction(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	label: &str,
	instances: Vec<CheckInstance>,
) -> Result<usize> {
	let total = instances.len();
	file_check_instances(
		db,
		InstancedCheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: Scope::Server(server_id),
			device_id: None,
			check: refs::REDACTION,
			title: Some("redaction incomplete"),
			instances,
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: Some(refs::REDACTION_DOC),
		},
		&|degraded| match degraded {
			[] => format!("Every redacting replica of {label} is fully masked"),
			[one] => format!(
				"Replica {} of {label} did not fully redact: {}",
				one.label,
				instance_field(one, "outcome"),
			),
			many => format!(
				"{} of {label}'s {total} redacting replicas did not fully redact: {}",
				many.len(),
				instance_labels(many),
			),
		},
	)
	.await?;
	Ok(1)
}

async fn file_migration(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	label: &str,
	instances: Vec<CheckInstance>,
) -> Result<usize> {
	let total = instances.len();
	file_check_instances(
		db,
		InstancedCheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: Scope::Server(server_id),
			device_id: None,
			check: refs::MIGRATION_TEST,
			title: Some("migration test overdue"),
			instances,
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: Some(refs::MIGRATION_TEST_DOC),
		},
		&|degraded| match degraded {
			[] => format!("Candidate versions have been migration-tested against {label}"),
			[one] => format!(
				"Candidate version not clean against {label}'s data: {} — {}",
				one.label,
				instance_why(one),
			),
			many => format!(
				"Candidate versions not clean against {label}'s data for {} of {total} replicas: {}",
				many.len(),
				instance_labels(many),
			),
		},
	)
	.await?;
	Ok(1)
}

/// A degraded instance's `why`, for a single-instance message.
fn instance_why(instance: &GradedInstance) -> String {
	instance_field(instance, "why")
}

fn instance_field(instance: &GradedInstance, field: &str) -> String {
	instance
		.detail
		.as_ref()
		.and_then(|d| d.get(field))
		.and_then(|v| v.as_str())
		.unwrap_or("no detail reported")
		.to_owned()
}

fn instance_labels(instances: &[GradedInstance]) -> String {
	instances
		.iter()
		.map(|i| i.label.as_str())
		.collect::<Vec<_>>()
		.join(", ")
}

/// The migration-test instance for one `migrate` declaration: whether the
/// server's candidate version has been tried against its latest snapshot within
/// the bound.
///
/// `None` when the server has nothing to be overdue about — no candidate
/// version, no snapshot to migrate, a verdict already recorded, or still inside
/// the bound. An instance that would say "fine" is not emitted, because a
/// declaration with nothing to test is not a passing test.
// spec: RST#alerting
async fn migration_instance(
	db: &mut AsyncPgConnection,
	declaration: &RestoreReplica,
	server: &crate::servers::Server,
	label: &str,
	latest: &HashMap<(Uuid, BackupType), BackupRun>,
	now: Timestamp,
	overdue_after: Option<PgDuration>,
) -> Result<Option<CheckInstance>> {
	let Some(version) = crate::migration_tests::candidate_for(db, server).await? else {
		return Ok(None);
	};
	let Some(run) = latest.get(&(server.id, declaration.r#type.clone())) else {
		return Ok(None);
	};
	let Some(snapshot_id) = run.snapshot_id.as_ref() else {
		return Ok(None);
	};

	let semver = version.as_semver();
	let verdict =
		crate::migration_tests::verdict_for(db, server.id, snapshot_id, version.id).await?;

	// A recorded verdict answers the check outright; without one, the bound
	// decides. Measured from when the snapshot landed, which is when it became
	// available to migrate, not how old the data inside it is.
	let (observed, why, failed_migration) = match verdict {
		Some(Some(migration)) => (
			CheckResult::Warning,
			format!("migration {migration} failed applying {semver}"),
			Some(migration),
		),
		Some(None) => (
			CheckResult::Passed,
			format!("migrations for {semver} applied"),
			None,
		),
		// Untested: only the overdue bound can make that a finding, and a
		// declaration without one is never overdue. A recorded verdict above
		// answers regardless of the bound — a failed migration is a fact about
		// the candidate version, not a deadline.
		None => match overdue_after {
			Some(bound) if now.duration_since(run.reported_at) > bound.0 => (
				CheckResult::Warning,
				format!("migrations for {semver} not tried within the overdue bound"),
				None,
			),
			_ => return Ok(None),
		},
	};

	Ok(Some(CheckInstance {
		label: label.to_owned(),
		observed,
		detail: Some(serde_json::json!({
			"target_version": semver.to_string(),
			"failed_migration": failed_migration,
			"snapshot_id": snapshot_id,
			"type": declaration.r#type.to_string(),
			"intent": declaration.intent.to_string(),
			"replica": declaration.name,
			"why": why,
		})),
	}))
}
