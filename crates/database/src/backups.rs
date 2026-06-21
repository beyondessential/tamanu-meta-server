//! Persistent state for the backup-credentials system: the control plane that
//! issues short-lived S3 creds to devices and owns repo maintenance. Backups
//! are keyed `(server, type)` — the repo/bucket is per-group and shared by all
//! the group's backup *types*, while the type (e.g. `tamanu-postgres`) is a
//! dimension on runs / issuances / requests / snapshots.
//!
//! This module owns the diesel models and the DB-layer helpers; the calling
//! logic (STS issuance, scheduler loops, operator UI) lives in the
//! public-server, `jobs`, and private-server components.

use std::collections::HashMap;

use commons_errors::{AppError, Result};
use commons_types::backup::{
	BackupConfigStatus, BackupPurpose, BackupRepoMode, BackupType, MaintenanceKind, RunOutcome,
};
use diesel::{
	dsl::now,
	prelude::*,
	result::{DatabaseErrorKind, Error as DieselError},
};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::pg_duration::PgDuration;

// ---------------------------------------------------------------------------
// RetentionPolicy — the kopia keep-* policy, modelled as a typed struct so the
// wire/openapi shape is concrete (not a raw JSON blob). Stored as JSONB on the
// per-(group,type) schedule and on the type defaults.
// ---------------------------------------------------------------------------

/// kopia `keep-*` retention policy. Org-minimum floors
/// (`keep_daily ≥ 7, keep_weekly ≥ 4, keep_monthly ≥ 6`) are enforced by
/// [`RetentionPolicy::validate_floor`] on create/update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RetentionPolicy {
	#[serde(default = "RetentionPolicy::default_keep_latest")]
	pub keep_latest: i32,
	pub keep_daily: i32,
	pub keep_weekly: i32,
	pub keep_monthly: i32,
	#[serde(default)]
	pub keep_annual: i32,
}

impl RetentionPolicy {
	pub const FLOOR_DAILY: i32 = 7;
	pub const FLOOR_WEEKLY: i32 = 4;
	pub const FLOOR_MONTHLY: i32 = 6;

	fn default_keep_latest() -> i32 {
		1
	}

	/// Reject a policy below the org-minimum floor. Returns
	/// [`AppError::BadRequest`] (→ 400) listing the violated field(s).
	pub fn validate_floor(&self) -> Result<()> {
		let mut violations = Vec::new();
		if self.keep_daily < Self::FLOOR_DAILY {
			violations.push(format!("keep_daily must be ≥ {}", Self::FLOOR_DAILY));
		}
		if self.keep_weekly < Self::FLOOR_WEEKLY {
			violations.push(format!("keep_weekly must be ≥ {}", Self::FLOOR_WEEKLY));
		}
		if self.keep_monthly < Self::FLOOR_MONTHLY {
			violations.push(format!("keep_monthly must be ≥ {}", Self::FLOOR_MONTHLY));
		}
		if violations.is_empty() {
			Ok(())
		} else {
			Err(AppError::BadRequest(format!(
				"retention below org minimum: {}",
				violations.join(", ")
			)))
		}
	}

	/// Parse a stored JSONB retention into the typed policy. Returns `None` for
	/// a JSON value that doesn't fit the shape (forward-compatible: a future
	/// extra key is ignored by serde, but a structurally wrong blob is dropped
	/// rather than erroring the whole listing).
	pub fn from_json(value: &JsonValue) -> Option<Self> {
		serde_json::from_value(value.clone()).ok()
	}

	pub fn to_json(&self) -> JsonValue {
		serde_json::to_value(self).expect("RetentionPolicy serializes")
	}
}

// ---------------------------------------------------------------------------
// server_group_backup_config — repo-level config, one row per configured group
// ---------------------------------------------------------------------------

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::server_group_backup_config)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroupBackupConfig {
	pub group_id: Uuid,
	pub bucket: String,
	pub prefix: String,
	pub target_role_arn: String,
	pub maintenance_role_arn: String,
	pub region: Option<String>,
	pub repo_password_ref: String,
	pub status: BackupConfigStatus,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	pub last_init_error: Option<String>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::server_group_backup_config)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewServerGroupBackupConfig {
	pub group_id: Uuid,
	pub bucket: String,
	pub prefix: String,
	pub target_role_arn: String,
	pub maintenance_role_arn: String,
	pub region: Option<String>,
	pub repo_password_ref: String,
	pub status: BackupConfigStatus,
	pub mode: BackupRepoMode,
}

impl ServerGroupBackupConfig {
	/// Fetch a group's backup config (absent → caller maps to 409).
	pub async fn get(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Option<Self>> {
		use crate::schema::server_group_backup_config::dsl;

		dsl::server_group_backup_config
			.filter(dsl::group_id.eq(group_id))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Fetch a group's backup config, mapping absent → 404
	/// (`AppError::DatabaseQuery(NotFound)`). For mutation handlers that
	/// require an existing config; `get` keeps the `Option` for the zero-state.
	pub async fn get_required(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Self> {
		Self::get(db, group_id)
			.await?
			.ok_or(AppError::DatabaseQuery(DieselError::NotFound))
	}

	/// All configured groups (the onboarding/stats listing).
	pub async fn list(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::server_group_backup_config::dsl;

		dsl::server_group_backup_config
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Create or replace a group's config (the operator onboarding write path).
	/// `created_at`/`updated_at` keep their DB defaults / auto-touch.
	pub async fn upsert(
		db: &mut AsyncPgConnection,
		new: NewServerGroupBackupConfig,
	) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::insert_into(dsl::server_group_backup_config)
			.values(&new)
			.on_conflict(dsl::group_id)
			.do_update()
			.set((
				dsl::bucket.eq(&new.bucket),
				dsl::prefix.eq(&new.prefix),
				dsl::target_role_arn.eq(&new.target_role_arn),
				dsl::maintenance_role_arn.eq(&new.maintenance_role_arn),
				dsl::region.eq(new.region.as_deref()),
				dsl::repo_password_ref.eq(&new.repo_password_ref),
				dsl::status.eq(new.status),
				dsl::mode.eq(new.mode),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Insert a fresh config row (the operator onboarding *create* path). A
	/// second create for the same group violates the PK and is surfaced as
	/// [`AppError::Conflict`] so the handler can return 409 ("already
	/// configured") rather than silently overwriting a live repo's config.
	pub async fn insert(
		db: &mut AsyncPgConnection,
		new: NewServerGroupBackupConfig,
	) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		match diesel::insert_into(dsl::server_group_backup_config)
			.values(&new)
			.returning(Self::as_select())
			.get_result(db)
			.await
		{
			Ok(row) => Ok(row),
			Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => Err(
				AppError::Conflict("group already has a backup config".into()),
			),
			Err(e) => Err(AppError::from(e)),
		}
	}

	/// Edit the non-structural config fields (region only — interval/retention
	/// live on the per-(group,type) schedule). Structural fields
	/// (bucket/role/mode) are immutable post-creation; the handler rejects
	/// those before calling here.
	pub async fn update_region(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		region: Option<&str>,
	) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::update(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.set((dsl::region.eq(region), dsl::updated_at.eq(now)))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Advance (or reset) the lifecycle status (repo-init flow).
	pub async fn set_status(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		status: BackupConfigStatus,
	) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::update(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.set(dsl::status.eq(status))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Record intent for the init Job: set `status='provisioning'` and clear
	/// any prior `last_init_error`. Idempotent — re-running it on an already-
	/// provisioning row is a no-op retry. The jobs-side init op observes the
	/// `provisioning` status and drives the row to `ready`.
	pub async fn mark_provisioning(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::update(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.set((
				dsl::status.eq(BackupConfigStatus::Provisioning),
				dsl::last_init_error.eq(None::<String>),
				dsl::updated_at.eq(now),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Record an init-Job failure: keep `status='provisioning'` and surface the
	/// error for the operator. (The jobs component calls this; included here so
	/// the operator-UI and tests can simulate the failure branch.)
	pub async fn set_last_init_error(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		error: &str,
	) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::update(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.set((
				dsl::status.eq(BackupConfigStatus::Provisioning),
				dsl::last_init_error.eq(Some(error)),
				dsl::updated_at.eq(now),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Delete a group's config row (decommission). Audit tables intentionally
	/// have no CASCADE here and the bucket persists (object-locked).
	pub async fn delete(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<()> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::delete(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// backup_type_defaults — canopy-wide per-type defaults
// ---------------------------------------------------------------------------

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::backup_type_defaults)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupTypeDefault {
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = Option<i64>, format = "int64")]
	pub default_interval: Option<PgDuration>,
	pub default_retention: JsonValue,
	pub auto_enable: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::backup_type_defaults)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewBackupTypeDefault {
	#[diesel(column_name = type_)]
	pub r#type: BackupType,
	pub default_interval: Option<PgDuration>,
	pub default_retention: JsonValue,
	pub auto_enable: bool,
}

impl BackupTypeDefault {
	pub async fn get(db: &mut AsyncPgConnection, r#type: &BackupType) -> Result<Option<Self>> {
		use crate::schema::backup_type_defaults::dsl;

		dsl::backup_type_defaults
			.filter(dsl::type_.eq(r#type.as_str()))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	pub async fn list(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::backup_type_defaults::dsl;

		dsl::backup_type_defaults
			.order(dsl::type_)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn upsert(db: &mut AsyncPgConnection, new: NewBackupTypeDefault) -> Result<Self> {
		use crate::schema::backup_type_defaults::dsl;

		diesel::insert_into(dsl::backup_type_defaults)
			.values(&new)
			.on_conflict(dsl::type_)
			.do_update()
			.set((
				dsl::default_interval.eq(new.default_interval),
				dsl::default_retention.eq(&new.default_retention),
				dsl::auto_enable.eq(new.auto_enable),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// server_backup_capabilities — what a server advertises it can back up
// ---------------------------------------------------------------------------

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::server_backup_capabilities)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerBackupCapability {
	pub server_id: Uuid,
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	pub enabled: bool,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub registered_at: Timestamp,
}

impl ServerBackupCapability {
	/// Register a capability advertised by bestool. `enabled_seed` is the
	/// type's `auto_enable` default and is applied only when the row is first
	/// created — an existing row keeps its operator-set `enabled`.
	pub async fn register(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		r#type: &BackupType,
		enabled_seed: bool,
	) -> Result<Self> {
		use crate::schema::server_backup_capabilities::dsl;

		diesel::insert_into(dsl::server_backup_capabilities)
			.values((
				dsl::server_id.eq(server_id),
				dsl::type_.eq(r#type.as_str()),
				dsl::enabled.eq(enabled_seed),
			))
			.on_conflict((dsl::server_id, dsl::type_))
			// Keep the existing (operator-set) enabled; no-op update so we can
			// RETURNING the existing row.
			.do_update()
			.set(dsl::server_id.eq(server_id))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Operator toggle for a `(server, type)`; durable, not re-seeded by the
	/// type default.
	pub async fn set_enabled(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		r#type: &BackupType,
		enabled: bool,
	) -> Result<Self> {
		use crate::schema::server_backup_capabilities::dsl;

		diesel::update(
			dsl::server_backup_capabilities
				.filter(dsl::server_id.eq(server_id))
				.filter(dsl::type_.eq(r#type.as_str())),
		)
		.set(dsl::enabled.eq(enabled))
		.returning(Self::as_select())
		.get_result(db)
		.await
		.map_err(AppError::from)
	}

	pub async fn list_for_server(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::server_backup_capabilities::dsl;

		dsl::server_backup_capabilities
			.filter(dsl::server_id.eq(server_id))
			.order(dsl::type_)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// All enabled `(server, type)` capabilities fleet-wide — the candidate set
	/// the scheduler / staleness scan starts from.
	pub async fn list_enabled(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::server_backup_capabilities::dsl;

		dsl::server_backup_capabilities
			.filter(dsl::enabled.eq(true))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Distinct backup types that are **enabled** on any non-archived server in
	/// the group — i.e. the types the group's repo is actively expected to
	/// hold. The scheduler resolves a per-type retention/cadence for each of
	/// these.
	pub async fn enabled_types_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Vec<BackupType>> {
		use crate::schema::{server_backup_capabilities as cap, servers};

		cap::table
			.inner_join(servers::table.on(servers::id.eq(cap::server_id)))
			.filter(servers::group_id.eq(group_id))
			.filter(servers::deleted_at.is_null())
			.filter(cap::enabled.eq(true))
			.select(cap::type_)
			.distinct()
			.load::<BackupType>(db)
			.await
			.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// server_group_backup_schedule — per-(group,type) schedule/retention overrides
// ---------------------------------------------------------------------------

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::server_group_backup_schedule)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroupBackupSchedule {
	pub group_id: Uuid,
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[schema(value_type = Option<i64>, format = "int64")]
	pub expected_interval: Option<PgDuration>,
	pub retention: Option<JsonValue>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::server_group_backup_schedule)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewServerGroupBackupSchedule {
	pub group_id: Uuid,
	#[diesel(column_name = type_)]
	pub r#type: BackupType,
	pub expected_interval: Option<PgDuration>,
	pub retention: Option<JsonValue>,
}

impl ServerGroupBackupSchedule {
	pub async fn get(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		r#type: &BackupType,
	) -> Result<Option<Self>> {
		use crate::schema::server_group_backup_schedule::dsl;

		dsl::server_group_backup_schedule
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::type_.eq(r#type.as_str()))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	pub async fn list_for_group(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::server_group_backup_schedule::dsl;

		dsl::server_group_backup_schedule
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::type_)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn upsert(
		db: &mut AsyncPgConnection,
		new: NewServerGroupBackupSchedule,
	) -> Result<Self> {
		use crate::schema::server_group_backup_schedule::dsl;

		diesel::insert_into(dsl::server_group_backup_schedule)
			.values(&new)
			.on_conflict((dsl::group_id, dsl::type_))
			.do_update()
			.set((
				dsl::expected_interval.eq(new.expected_interval),
				dsl::retention.eq(&new.retention),
				dsl::updated_at.eq(now),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// backup_credential_issuances — audit log of every STS issuance
// ---------------------------------------------------------------------------

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::backup_credential_issuances)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupCredentialIssuance {
	pub id: i64,
	pub device_id: Uuid,
	pub group_id: Uuid,
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub issued_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub expires_at: Timestamp,
	pub purpose: BackupPurpose,
	pub sts_assumed_role: String,
	pub sts_request_id: Option<String>,
	pub access_key_id: Option<String>,
	pub bucket: String,
	pub prefix: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::backup_credential_issuances)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewBackupCredentialIssuance {
	pub device_id: Uuid,
	pub group_id: Uuid,
	#[diesel(column_name = type_)]
	pub r#type: BackupType,
	#[diesel(serialize_as = jiff_diesel::Timestamp)]
	pub expires_at: Timestamp,
	pub purpose: BackupPurpose,
	pub sts_assumed_role: String,
	pub sts_request_id: Option<String>,
	pub access_key_id: Option<String>,
	/// Snapshot of `bucket` at issuance time (not an FK back to config).
	pub bucket: String,
	/// Snapshot of `prefix` at issuance time.
	pub prefix: String,
}

impl BackupCredentialIssuance {
	/// Record an issuance (public-server, after a successful AssumeRole).
	/// `device_id`/`group_id`/bucket/prefix are resolved by the caller, not
	/// read from a client body.
	pub async fn record(
		db: &mut AsyncPgConnection,
		new: NewBackupCredentialIssuance,
	) -> Result<Self> {
		use crate::schema::backup_credential_issuances::dsl;

		diesel::insert_into(dsl::backup_credential_issuances)
			.values(new)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Recent issuances for a device, newest-first (served by the
	/// `(device_id, issued_at DESC)` index).
	pub async fn list_for_device(
		db: &mut AsyncPgConnection,
		device_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_credential_issuances::dsl;

		dsl::backup_credential_issuances
			.filter(dsl::device_id.eq(device_id))
			.order(dsl::issued_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Recent issuances for a group, newest-first.
	pub async fn list_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_credential_issuances::dsl;

		dsl::backup_credential_issuances
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::issued_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// backup_runs — what bestool reported per run (client-minted UUID PK)
// ---------------------------------------------------------------------------

#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::backup_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRun {
	pub id: Uuid,
	pub device_id: Uuid,
	pub group_id: Uuid,
	pub server_id: Option<Uuid>,
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	pub purpose: BackupPurpose,
	pub outcome: RunOutcome,
	pub error: Option<String>,
	pub bytes_uploaded: Option<i64>,
	pub snapshot_id: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub reported_at: Timestamp,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::backup_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewBackupRun {
	/// The run-uuid bestool minted at run start (becomes the PK).
	pub id: Uuid,
	pub device_id: Uuid,
	pub group_id: Uuid,
	pub server_id: Option<Uuid>,
	#[diesel(column_name = type_)]
	pub r#type: BackupType,
	pub purpose: BackupPurpose,
	pub outcome: RunOutcome,
	pub error: Option<String>,
	pub bytes_uploaded: Option<i64>,
	pub snapshot_id: Option<String>,
}

impl BackupRun {
	/// Record a reported run. A duplicate `id` (the client-minted run-uuid)
	/// fails its own insert; that PK violation is surfaced as
	/// [`AppError::Conflict`] so the endpoint can choose 409 vs idempotent 204.
	pub async fn record(db: &mut AsyncPgConnection, new: NewBackupRun) -> Result<Self> {
		use crate::schema::backup_runs::dsl;

		let id = new.id;
		match diesel::insert_into(dsl::backup_runs)
			.values(&new)
			.returning(Self::as_select())
			.get_result(db)
			.await
		{
			Ok(run) => Ok(run),
			Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => Err(
				AppError::Conflict(format!("backup run {id} already reported")),
			),
			Err(e) => Err(AppError::from(e)),
		}
	}

	/// Latest successful *backup* (never restore) for a `(server, type)` — the
	/// staleness anchor. Filters `purpose='backup'` + `outcome='success'` so a
	/// recent successful restore can't reset backup staleness.
	pub async fn latest_success_for_server(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		r#type: &BackupType,
	) -> Result<Option<Self>> {
		use crate::schema::backup_runs::dsl;

		dsl::backup_runs
			.filter(dsl::server_id.eq(server_id))
			.filter(dsl::type_.eq(r#type.as_str()))
			.filter(dsl::purpose.eq(BackupPurpose::Backup))
			.filter(dsl::outcome.eq(RunOutcome::Success))
			.order(dsl::reported_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Latest successful backup per `(server, type)` within a group — the bulk
	/// staleness-scan input. Keyed `(server_id, type)`; rows with a NULL
	/// `server_id` are skipped (they can't be attributed to a server).
	pub async fn latest_success_by_server_type_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<HashMap<(Uuid, BackupType), Self>> {
		use crate::schema::backup_runs::dsl;

		let rows: Vec<Self> = dsl::backup_runs
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::purpose.eq(BackupPurpose::Backup))
			.filter(dsl::outcome.eq(RunOutcome::Success))
			.filter(dsl::server_id.is_not_null())
			.distinct_on((dsl::server_id, dsl::type_))
			.order_by((dsl::server_id, dsl::type_, dsl::reported_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)?;

		Ok(rows
			.into_iter()
			.filter_map(|r| r.server_id.map(|sid| ((sid, r.r#type.clone()), r)))
			.collect())
	}

	/// Recent runs for a group, newest-first (stats panel).
	pub async fn list_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_runs::dsl;

		dsl::backup_runs
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::reported_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// backup_maintenance_runs — Canopy-owned maintenance outcomes (per-group)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_maintenance_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupMaintenanceRun {
	pub id: i64,
	pub group_id: Uuid,
	pub kind: MaintenanceKind,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub started_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub finished_at: Option<Timestamp>,
	/// NULL while running.
	pub outcome: Option<RunOutcome>,
	pub error: Option<String>,
	pub bytes_reclaimed: Option<i64>,
}

impl BackupMaintenanceRun {
	/// Open a maintenance-run row at Job start; returns the new id for the
	/// matching [`finish`](Self::finish).
	pub async fn start(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		kind: MaintenanceKind,
	) -> Result<i64> {
		use crate::schema::backup_maintenance_runs::dsl;

		diesel::insert_into(dsl::backup_maintenance_runs)
			.values((dsl::group_id.eq(group_id), dsl::kind.eq(kind)))
			.returning(dsl::id)
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Close a maintenance run with its outcome.
	pub async fn finish(
		db: &mut AsyncPgConnection,
		id: i64,
		outcome: RunOutcome,
		error: Option<String>,
		bytes_reclaimed: Option<i64>,
	) -> Result<()> {
		use crate::schema::backup_maintenance_runs::dsl;

		diesel::update(dsl::backup_maintenance_runs.filter(dsl::id.eq(id)))
			.set((
				dsl::finished_at.eq(now),
				dsl::outcome.eq(Some(outcome)),
				dsl::error.eq(error),
				dsl::bytes_reclaimed.eq(bytes_reclaimed),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	pub async fn list_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_maintenance_runs::dsl;

		dsl::backup_maintenance_runs
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::started_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Whether a run row still exists and is open (`outcome IS NULL`). Used by
	/// the scheduler's crash-detection to mark a run failed when its Job
	/// finished without ever reporting.
	pub async fn is_open(db: &mut AsyncPgConnection, id: i64) -> Result<bool> {
		use crate::schema::backup_maintenance_runs::dsl;

		let open: Option<bool> = dsl::backup_maintenance_runs
			.filter(dsl::id.eq(id))
			.select(dsl::outcome.is_null())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?;
		Ok(open.unwrap_or(false))
	}
}

// ---------------------------------------------------------------------------
// backup_repo_snapshots — ground-truth inventory from the inspection Job
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_repo_snapshots)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRepoSnapshot {
	pub group_id: Uuid,
	pub source: String,
	pub server_id: Option<Uuid>,
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = Option<String>)]
	pub r#type: Option<BackupType>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub latest_snapshot_at: Option<Timestamp>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub observed_at: Timestamp,
}

impl BackupRepoSnapshot {
	/// Upsert the inventory for one kopia source (the inspection Job calls this
	/// per source, refreshing `latest_snapshot_at`/`observed_at` in place).
	pub async fn upsert(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		source: &str,
		server_id: Option<Uuid>,
		r#type: Option<&BackupType>,
		latest_snapshot_at: Option<Timestamp>,
	) -> Result<()> {
		use crate::schema::backup_repo_snapshots::dsl;

		let type_str = r#type.map(BackupType::as_str);
		let latest = jiff_diesel::NullableTimestamp::from(latest_snapshot_at);

		diesel::insert_into(dsl::backup_repo_snapshots)
			.values((
				dsl::group_id.eq(group_id),
				dsl::source.eq(source),
				dsl::server_id.eq(server_id),
				dsl::type_.eq(type_str),
				dsl::latest_snapshot_at.eq(latest),
			))
			.on_conflict((dsl::group_id, dsl::source))
			.do_update()
			.set((
				dsl::server_id.eq(server_id),
				dsl::type_.eq(type_str),
				dsl::latest_snapshot_at.eq(latest),
				dsl::observed_at.eq(now),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	pub async fn list_for_group(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::backup_repo_snapshots::dsl;

		dsl::backup_repo_snapshots
			.filter(dsl::group_id.eq(group_id))
			.order(dsl::source)
			.load(db)
			.await
			.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// backup_repo_stats — cached repo + bucket stats (per-group, two writers)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_repo_stats)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRepoStats {
	pub group_id: Uuid,
	pub snapshot_count: Option<i32>,
	pub source_count: Option<i32>,
	pub logical_bytes: Option<i64>,
	pub physical_bytes: Option<i64>,
	pub bucket_bytes: Option<i64>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub observed_at: Timestamp,
}

impl BackupRepoStats {
	pub async fn get(db: &mut AsyncPgConnection, group_id: Uuid) -> Result<Option<Self>> {
		use crate::schema::backup_repo_stats::dsl;

		dsl::backup_repo_stats
			.filter(dsl::group_id.eq(group_id))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Writer 1: the read-only inspection Job. Touches only the repo-derived
	/// fields (+ `observed_at`), never `bucket_bytes`.
	pub async fn upsert_repo_fields(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		snapshot_count: Option<i32>,
		source_count: Option<i32>,
		logical_bytes: Option<i64>,
		physical_bytes: Option<i64>,
	) -> Result<()> {
		use crate::schema::backup_repo_stats::dsl;

		diesel::insert_into(dsl::backup_repo_stats)
			.values((
				dsl::group_id.eq(group_id),
				dsl::snapshot_count.eq(snapshot_count),
				dsl::source_count.eq(source_count),
				dsl::logical_bytes.eq(logical_bytes),
				dsl::physical_bytes.eq(physical_bytes),
			))
			.on_conflict(dsl::group_id)
			.do_update()
			.set((
				dsl::snapshot_count.eq(snapshot_count),
				dsl::source_count.eq(source_count),
				dsl::logical_bytes.eq(logical_bytes),
				dsl::physical_bytes.eq(physical_bytes),
				dsl::observed_at.eq(now),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Writer 2: the S3-metrics task. Touches only `bucket_bytes`
	/// (+ `observed_at`), never the repo-derived fields.
	pub async fn upsert_bucket_bytes(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		bucket_bytes: Option<i64>,
	) -> Result<()> {
		use crate::schema::backup_repo_stats::dsl;

		diesel::insert_into(dsl::backup_repo_stats)
			.values((
				dsl::group_id.eq(group_id),
				dsl::bucket_bytes.eq(bucket_bytes),
			))
			.on_conflict(dsl::group_id)
			.do_update()
			.set((dsl::bucket_bytes.eq(bucket_bytes), dsl::observed_at.eq(now)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// backup_requests — pending operator one-off "backup now" flags
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRequest {
	pub server_id: Uuid,
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	pub purpose: BackupPurpose,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub requested_at: Timestamp,
	pub requested_by: Option<String>,
}

impl BackupRequest {
	/// Enqueue (or refresh) a one-off request for a `(server, type, purpose)`.
	pub async fn enqueue(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		r#type: &BackupType,
		purpose: BackupPurpose,
		requested_by: Option<&str>,
	) -> Result<()> {
		use crate::schema::backup_requests::dsl;

		diesel::insert_into(dsl::backup_requests)
			.values((
				dsl::server_id.eq(server_id),
				dsl::type_.eq(r#type.as_str()),
				dsl::purpose.eq(purpose),
				dsl::requested_by.eq(requested_by),
			))
			.on_conflict((dsl::server_id, dsl::type_, dsl::purpose))
			.do_update()
			.set((
				dsl::requested_at.eq(now),
				dsl::requested_by.eq(requested_by),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Clear a pending request (called when the run is reported).
	pub async fn clear(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		r#type: &BackupType,
		purpose: BackupPurpose,
	) -> Result<()> {
		use crate::schema::backup_requests::dsl;

		diesel::delete(
			dsl::backup_requests
				.filter(dsl::server_id.eq(server_id))
				.filter(dsl::type_.eq(r#type.as_str()))
				.filter(dsl::purpose.eq(purpose)),
		)
		.execute(db)
		.await
		.map_err(AppError::from)?;
		Ok(())
	}

	pub async fn pending_for_server(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_requests::dsl;

		dsl::backup_requests
			.filter(dsl::server_id.eq(server_id))
			.order((dsl::type_, dsl::purpose))
			.load(db)
			.await
			.map_err(AppError::from)
	}
}
