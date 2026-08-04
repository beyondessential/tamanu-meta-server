//! Managed restore replicas (RST): the control-plane state for driving an
//! external restore consumer. Operators declare which replicas should exist
//! ([`RestoreReplica`]); consumers register the intents they can satisfy
//! ([`RestoreConsumerCapability`]). The worklist expansion, credential issuance,
//! and restore-health ingest live in the public-server and `jobs` components.

use std::collections::{HashMap, hash_map::Entry};

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
use crate::issues::{CheckFiling, Scope, file_check};
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

		let existing = Self::get(db, id).await?;
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

		let scope_changed = existing.group_id != result.group_id
			|| existing.server_id != result.server_id
			|| existing.r#type != result.r#type
			|| existing.intent != result.intent;
		// Disabling drops the declaration out of the overdue sweep exactly as
		// deleting it does (`sweep_overdue` filters `enabled = true`), and a
		// disabled replica generates no consumer work, so `record_report` can't
		// clear the alert either. Left alone, the alert and its incident stay
		// open forever.
		let disabled = existing.enabled && !result.enabled;
		if scope_changed || disabled {
			recover_old_scope_alerts(
				db,
				existing.group_id,
				existing.server_id,
				&existing.r#type,
				&existing.intent,
				existing.redacts,
			)
			.await?;
		}

		Ok(result)
	}

	/// Delete a declaration and recover any active restore-verification alert
	/// keyed to its `(server, type, intent)` scope — the overdue sweep only
	/// walks current declarations, so an alert left behind by a deleted one
	/// would otherwise never clear. Runs in a single transaction so a failure
	/// partway can't leave the row deleted with its alerts unrecovered.
	///
	/// Restore-health reports the declaration collected are retained: the FK
	/// from `backup_restore_checks` is `ON DELETE SET NULL`, so each report
	/// survives with its group, server, type, and intent, no longer attached to
	/// a declaration.
	pub async fn delete(db: &mut AsyncPgConnection, id: Uuid) -> Result<()> {
		db.transaction::<_, AppError, _>(async |conn| {
			use crate::schema::restore_replicas::dsl;
			let existing = Self::get(conn, id).await?;
			let n = diesel::delete(dsl::restore_replicas.filter(dsl::id.eq(id)))
				.execute(conn)
				.await?;
			if n == 0 {
				return Err(AppError::DatabaseQuery(DieselError::NotFound));
			}
			recover_old_scope_alerts(
				conn,
				existing.group_id,
				existing.server_id,
				&existing.r#type,
				&existing.intent,
				existing.redacts,
			)
			.await?;
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

/// The stable alert ref for one replica's restore-health. Per
/// `(server, type, intent)` so each replica recovers independently (one
/// intent's healthy report must not clear another's failure on the same
/// server).
fn restore_verification_ref(r#type: &BackupType, intent: &RestoreIntent) -> String {
	// The check is server-scoped (the per-server dimension is the filing's
	// server_id), so the name is stable per (type, intent) and shared across
	// the fleet — one catalog policy, not one single-use name per server.
	format!("{}:{}:{}", refs::RESTORE_VERIFICATION, r#type, intent)
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

/// The stable check ref for one replica's redaction, keyed the same way
/// restore-verification is so the catalog holds one policy per replica shape
/// rather than one per server.
fn redaction_ref(r#type: &BackupType, intent: &RestoreIntent) -> String {
	format!("{}:{}:{}", refs::REDACTION, r#type, intent)
}

/// What a consumer's masking did to the replica a report is about.
#[derive(Debug, Clone)]
struct ReportedRedaction {
	outcome: RedactionOutcome,
	manifest_version: Option<String>,
	columns_masked: Option<i64>,
	columns_skipped: Option<i64>,
	error: Option<String>,
}

/// Raise or recover the server's redaction check.
///
/// A warning that does not escalate. The deployment is healthy and its data
/// is where it should be; the finding is that a replica made from that data
/// is not as safe to hand out as it was declared to be, which belongs to
/// whoever gave out the replica rather than to whoever is on call for
/// outages.
// spec: RST#alerting
async fn file_redaction_outcome(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	r#type: &BackupType,
	intent: &RestoreIntent,
	redaction: &ReportedRedaction,
) -> Result<()> {
	let r#ref = redaction_ref(r#type, intent);
	let detail = serde_json::json!({
		"outcome": redaction.outcome.to_string(),
		"manifest_version": redaction.manifest_version,
		"columns_masked": redaction.columns_masked,
		"columns_skipped": redaction.columns_skipped,
		"error": redaction.error,
	});

	let (observed, title, message) = match redaction.outcome {
		RedactionOutcome::Complete => (
			CheckResult::Passed,
			None,
			format!("Replica of server {server_id} redacted: {type} / {intent}"),
		),
		RedactionOutcome::Partial => {
			let skipped = redaction
				.columns_skipped
				.map(|n| format!("{n} columns"))
				.unwrap_or_else(|| "some columns".into());
			(
				CheckResult::Warning,
				Some("redaction partial"),
				format!(
					"Replica of server {server_id} is live with {skipped} unmasked: {type} / {intent}"
				),
			)
		}
		RedactionOutcome::Failed => {
			let why = redaction
				.error
				.clone()
				.unwrap_or_else(|| "no reason given".into());
			(
				CheckResult::Warning,
				Some("redaction failed"),
				format!(
					"Replica of server {server_id} is held on its previous data, unredacted: {type} / {intent}: {why}"
				),
			)
		}
	};

	file_check(
		db,
		CheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: Scope::Server(server_id),
			device_id: None,
			check: &r#ref,
			observed,
			title,
			message: &message,
			detail: Some(detail),
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: Some(refs::REDACTION_DOC),
		},
	)
	.await?;
	Ok(())
}

/// Recover any active restore-verification alert keyed to a declaration's old
/// `(server, type, intent)`, called when the declaration stops covering that
/// scope (it was deleted, or its scope moved elsewhere). Mirrors the recovery
/// [`BackupRestoreCheck::record_report`] performs for a healthy report —
/// [`raise_group_event`] with `active: false`. If the old key is still overdue
/// under some other declaration, the next [`sweep_overdue`] pass re-raises it.
async fn recover_old_scope_alerts(
	db: &mut AsyncPgConnection,
	old_group_id: Uuid,
	old_server_id: Option<Uuid>,
	old_type: &BackupType,
	old_intent: &RestoreIntent,
	old_redacts: bool,
) -> Result<()> {
	let servers = match old_server_id {
		Some(sid) => vec![sid],
		None => crate::servers::Server::list_live_in_group(db, old_group_id)
			.await?
			.into_iter()
			.map(|s| s.id)
			.collect(),
	};

	for sid in servers {
		let r#ref = restore_verification_ref(old_type, old_intent);
		file_check(
			db,
			CheckFiling {
				source: crate::statuses::CANOPY_SOURCE,
				scope: Scope::Server(sid), device_id: None,
				check: &r#ref,
				observed: CheckResult::Passed,
				title: None,
				message: &format!(
					"Restore verification no longer tracked at this scope: {old_type} / {old_intent} for server {sid}"
				),
				detail: None,
				default_ceiling: CheckResult::Failed,
				default_escalates: false,
				documentation: Some(refs::RESTORE_VERIFICATION_DOC),
			},
		)
		.await?;

		// Only a declaration that redacted has a redaction check to clear;
		// recovering one for every declaration would seed the catalog with
		// checks that were never raised.
		if old_redacts {
			let r#ref = redaction_ref(old_type, old_intent);
			file_check(
				db,
				CheckFiling {
					source: crate::statuses::CANOPY_SOURCE,
					scope: Scope::Server(sid),
					device_id: None,
					check: &r#ref,
					observed: CheckResult::Passed,
					title: None,
					message: &format!(
						"Redaction no longer tracked at this scope: {old_type} / {old_intent} for server {sid}"
					),
					detail: None,
					default_ceiling: CheckResult::Warning,
					default_escalates: false,
					documentation: Some(refs::REDACTION_DOC),
				},
			)
			.await?;
		}
	}

	Ok(())
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
	/// Record a restore-health report and raise or recover its group-level
	/// alert. A success-and-healthy report recovers the replica's
	/// `restore-verification` issue; any other outcome raises it (`Error`,
	/// group-level, pages regardless of `is_monitored`).
	pub async fn record_report(
		db: &mut AsyncPgConnection,
		new: NewBackupRestoreCheck,
	) -> Result<i64> {
		use crate::schema::backup_restore_checks::dsl;

		let healthy = new.outcome == RunOutcome::Success && new.replica_healthy;
		let server_id = new.server_id;
		let r#type = new.r#type.clone();
		let intent = new.intent.clone();
		let error = new.error.clone();
		let snapshot_id = new.snapshot_id.clone();
		let redaction = new.redaction_outcome.map(|outcome| ReportedRedaction {
			outcome,
			manifest_version: new.redaction_manifest_version.clone(),
			columns_masked: new.redaction_columns_masked,
			columns_skipped: new.redaction_columns_skipped,
			error: new.redaction_error.clone(),
		});

		let check_id: i64 = diesel::insert_into(dsl::backup_restore_checks)
			.values(new)
			.returning(dsl::id)
			.get_result(db)
			.await?;

		// Restore-health is attributed per server; a report without one is
		// recorded but raises no group-level incident.
		if let Some(sid) = server_id {
			let r#ref = restore_verification_ref(&r#type, &intent);
			if healthy {
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Server(sid),
						device_id: None,
						check: &r#ref,
						observed: CheckResult::Passed,
						title: None,
						message: &format!(
							"Restore verification healthy: {type} / {intent} for server {sid}"
						),
						detail: None,
						default_ceiling: CheckResult::Failed,
						default_escalates: false,
						documentation: Some(refs::RESTORE_VERIFICATION_DOC),
					},
				)
				.await?;
			} else {
				let error_detail =
					error.unwrap_or_else(|| "restored database did not come up healthy".into());
				let snap = snapshot_id
					.clone()
					.map(|s| format!(" (snapshot {s})"))
					.unwrap_or_default();
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Server(sid),
						device_id: None,
						check: &r#ref,
						observed: CheckResult::Failed,
						title: Some("restore verification failed"),
						message: &format!(
							"Restore verification failed: {type} / {intent} for server {sid}{snap}: {error_detail}"
						),
						detail: Some(serde_json::json!({
							"type": r#type.to_string(),
							"intent": intent.to_string(),
							"snapshot_id": snapshot_id,
						})),
						default_ceiling: CheckResult::Failed,
						default_escalates: false,
						documentation: Some(refs::RESTORE_VERIFICATION_DOC),
					},
				)
				.await?;
			}

			// Redaction is its own signal: a replica can restore healthily and
			// still come up with data that was meant to be masked and isn't.
			if let Some(redaction) = redaction {
				file_redaction_outcome(db, sid, &r#type, &intent, &redaction).await?;
			}
		}
		Ok(check_id)
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

	let declarations: Vec<RestoreReplica> = dsl::restore_replicas
		.select(RestoreReplica::as_select())
		.filter(dsl::enabled.eq(true))
		.filter(dsl::overdue_after.is_not_null())
		.load(db)
		.await?;

	// Per-consumer descriptors (to read semantics) and per-group health anchors.
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
	let mut filed = 0usize;

	for d in declarations {
		let Some(overdue_after) = d.overdue_after else {
			continue;
		};

		// Skip declarations the consumer can't satisfy — those are gaps, not
		// restore-health incidents. Only `check` intents are held to a bound.
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
		}

		for server in servers {
			let sid = server.id;

			// A `migrate` intent is overdue on its own terms: the question is
			// whether the candidate version has been tried against the latest
			// snapshot, not whether the replica restored.
			if migrates {
				if let Some(filed_one) = sweep_migration_overdue(
					db,
					&d,
					&server,
					&latest_snapshot_cache[&d.group_id],
					now,
					overdue_after,
				)
				.await?
				{
					filed += filed_one;
				}
				continue;
			}

			let key = (sid, d.r#type.clone(), d.intent.clone());
			let overdue = if once {
				// A `once` intent is overdue only when a snapshot exists to verify,
				// it is not the last one verified, and it has stood past the bound.
				match latest_snapshot_cache[&d.group_id].get(&(sid, d.r#type.clone())) {
					None => false,
					Some(run) => {
						let verified = verified_snapshot_cache[&d.group_id].get(&key);
						let already = matches!(
							(verified, run.snapshot_id.as_ref()),
							(Some(v), Some(s)) if v == s
						);
						// Measured from the report, not `run.anchor()`: the question is
						// how long this snapshot has gone unverified since it became
						// available to verify, which is when it landed — not how old
						// the data inside it is.
						!already && now.duration_since(run.reported_at) > overdue_after.0
					}
				}
			} else {
				match healthy_cache[&d.group_id].get(&key) {
					Some(last) => now.duration_since(*last) > overdue_after.0,
					None => true,
				}
			};
			if !overdue {
				continue;
			}
			let r#ref = restore_verification_ref(&d.r#type, &d.intent);
			let message = if once {
				format!(
					"Latest snapshot for {} / {} on server {sid} has not been verified within its overdue bound",
					d.r#type, d.intent
				)
			} else {
				format!(
					"No healthy restore verification for {} / {} on server {sid} within its overdue bound",
					d.r#type, d.intent
				)
			};
			file_check(
				db,
				CheckFiling {
					source: crate::statuses::CANOPY_SOURCE,
					scope: Scope::Server(sid),
					device_id: None,
					check: &r#ref,
					observed: CheckResult::Failed,
					title: Some("restore verification overdue"),
					message: &message,
					detail: Some(serde_json::json!({
						"type": d.r#type.to_string(),
						"intent": d.intent.to_string(),
						"latest_snapshot_unverified": once,
					})),
					default_ceiling: CheckResult::Failed,
					default_escalates: false,
					documentation: Some(refs::RESTORE_VERIFICATION_DOC),
				},
			)
			.await?;
			filed += 1;
		}
	}

	Ok(filed)
}

/// Whether a `migrate` declaration's server has left its candidate version
/// untested past the bound, filing the migration-test check when it has.
///
/// Returns the number of checks filed, or `None` when the server has nothing to
/// be overdue about: no candidate version, or no snapshot to migrate.
// spec: RST#alerting
async fn sweep_migration_overdue(
	db: &mut AsyncPgConnection,
	declaration: &RestoreReplica,
	server: &crate::servers::Server,
	latest: &HashMap<(Uuid, BackupType), BackupRun>,
	now: Timestamp,
	overdue_after: PgDuration,
) -> Result<Option<usize>> {
	let Some(version) = crate::migration_tests::candidate_for(db, server).await? else {
		return Ok(None);
	};
	let Some(run) = latest.get(&(server.id, declaration.r#type.clone())) else {
		return Ok(None);
	};
	let Some(snapshot_id) = run.snapshot_id.as_ref() else {
		return Ok(None);
	};

	if crate::migration_tests::has_verdict(db, server.id, snapshot_id, version.id).await? {
		return Ok(None);
	}
	// Measured from when the snapshot landed, which is when it became available
	// to migrate, not how old the data inside it is.
	if now.duration_since(run.reported_at) <= overdue_after.0 {
		return Ok(None);
	}

	let semver = version.as_semver();
	file_check(
		db,
		CheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: Scope::Server(server.id),
			device_id: None,
			check: &format!(
				"{}:{}:{}",
				refs::MIGRATION_TEST, declaration.r#type, declaration.intent
			),
			observed: CheckResult::Warning,
			title: Some("migration test overdue"),
			message: &format!(
				"Migrations for {semver} have not been tried against server {}'s latest snapshot within its overdue bound",
				server.id
			),
			detail: Some(serde_json::json!({
				"target_version": semver.to_string(),
				"snapshot_id": snapshot_id,
				"type": declaration.r#type.to_string(),
				"intent": declaration.intent.to_string(),
			})),
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: Some(refs::MIGRATION_TEST_DOC),
		},
	)
	.await?;

	Ok(Some(1))
}
