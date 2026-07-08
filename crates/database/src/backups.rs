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
	BackupConfigStatus, BackupPlacement, BackupPurpose, BackupRepoMode, BackupType,
	MaintenanceKind, RunOutcome,
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

/// How many backups to keep at each retention tier once older snapshots are
/// pruned. The organization enforces minimum floors (at least 7 daily, 4
/// weekly, and 6 monthly) on write, unless the containing schedule or default
/// explicitly opts out via its `allow_below_floor` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RetentionPolicy {
	/// Number of most-recent backups to always keep regardless of age.
	/// Defaults to 1.
	#[serde(default = "RetentionPolicy::default_keep_latest")]
	pub keep_latest: i32,
	/// Number of most-recent daily backups to keep. Must be at least 7 unless
	/// the retention floor is bypassed.
	pub keep_daily: i32,
	/// Number of most-recent weekly backups to keep. Must be at least 4 unless
	/// the retention floor is bypassed.
	pub keep_weekly: i32,
	/// Number of most-recent monthly backups to keep. Must be at least 6
	/// unless the retention floor is bypassed.
	pub keep_monthly: i32,
	/// Number of most-recent annual backups to keep. Defaults to 0 (no annual
	/// retention).
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

/// Backup repository configuration for a server group: which bucket/prefix
/// the group's backups are stored in, the IAM roles used to access it, and
/// the current provisioning lifecycle state.
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::server_group_backup_config)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroupBackupConfig {
	/// ID of the server group this backup configuration belongs to.
	pub group_id: Uuid,
	/// Name of the S3 bucket backups are stored in.
	pub bucket: String,
	/// Key prefix within the bucket under which this group's backup
	/// repository lives. Empty string means the repository sits at the
	/// bucket root.
	pub prefix: String,
	/// ARN of the IAM role devices assume to upload their own backups
	/// (write-only; no delete permission).
	pub target_role_arn: String,
	/// ARN of the IAM role used to run repository maintenance, inspection,
	/// and metrics collection (full read/write/delete permission).
	pub maintenance_role_arn: String,
	/// AWS region the bucket lives in, if known.
	pub region: Option<String>,
	/// Reference to where the repository's encryption passphrase is stored.
	/// Never the passphrase itself.
	pub repo_password_ref: String,
	/// Current lifecycle state of this configuration.
	pub status: BackupConfigStatus,
	/// When this configuration was first created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// When this configuration was last updated.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// How the repository's passphrase was sourced: generated fresh for a new
	/// repository, or supplied by the operator to connect to an existing one.
	#[schema(value_type = String)]
	pub mode: BackupRepoMode,
	/// Error message from the most recent failed provisioning attempt, if
	/// any.
	pub last_init_error: Option<String>,
	/// Where the backup bucket lives and who provisioned it: an externally
	/// provisioned account the deployment brought itself, or an
	/// automatically provisioned bucket in a shared account.
	#[schema(value_type = String)]
	pub placement: BackupPlacement,
	/// Set when an operator has requested a one-off full maintenance run; the
	/// scheduler honors it on its next tick (bypassing the cadence slot) and
	/// clears it once the run is spawned. `None` = no pending request.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub force_full_maintenance_at: Option<Timestamp>,
	/// Who requested the pending full-maintenance run, if any.
	pub force_full_maintenance_by: Option<String>,
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
	pub placement: BackupPlacement,
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

	/// Update the mutable structural fields for the machine config-as-a-resource
	/// upsert: the two role ARNs + region. Bucket/prefix/mode stay immutable (the
	/// handler rejects changes to those before calling here).
	pub async fn update_roles_region(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		target_role_arn: &str,
		maintenance_role_arn: &str,
		region: Option<&str>,
	) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::update(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.set((
				dsl::target_role_arn.eq(target_role_arn),
				dsl::maintenance_role_arn.eq(maintenance_role_arn),
				dsl::region.eq(region),
				dsl::updated_at.eq(now),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Flag a one-off full maintenance run for the scheduler to pick up on its
	/// next tick (bypassing the cadence slot). Idempotent: re-requesting refreshes
	/// the timestamp/requester. `updated_at` is deliberately not touched — this is
	/// operator intent, not a config edit.
	pub async fn request_full_maintenance(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		requested_by: Option<&str>,
	) -> Result<Self> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::update(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.set((
				dsl::force_full_maintenance_at.eq(now),
				dsl::force_full_maintenance_by.eq(requested_by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Clear a pending full-maintenance request — called both when the scheduler
	/// spawns the run and when an operator cancels a not-yet-picked-up request.
	pub async fn clear_full_maintenance_request(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<()> {
		use crate::schema::server_group_backup_config::dsl;

		diesel::update(dsl::server_group_backup_config.filter(dsl::group_id.eq(group_id)))
			.set((
				dsl::force_full_maintenance_at.eq(None::<jiff_diesel::Timestamp>),
				dsl::force_full_maintenance_by.eq(None::<String>),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
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

/// Canopy-wide default schedule and retention for a backup type. Any group
/// that doesn't set its own override for the type inherits these values.
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::backup_type_defaults)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupTypeDefault {
	/// The backup type this default applies to (e.g. `tamanu-postgres`).
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Default interval, in seconds, between scheduled backups of this type.
	/// `None` means manual-only (no schedule).
	#[schema(value_type = Option<i64>, format = "int64")]
	pub default_interval: Option<PgDuration>,
	/// Default retention policy for this type: how many backups to keep at
	/// each retention tier.
	pub default_retention: JsonValue,
	/// Whether a server that advertises this type has it enabled
	/// automatically, without an operator opting in.
	pub auto_enable: bool,
	/// Opt out of the org retention floor for this type's default (dangerous):
	/// the floor is neither validated on write nor enforced on resolve.
	pub allow_below_floor: bool,
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
	pub allow_below_floor: bool,
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
				dsl::allow_below_floor.eq(new.allow_below_floor),
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

/// A backup type that a server has advertised it can run, and whether it's
/// currently enabled for that server.
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::server_backup_capabilities)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerBackupCapability {
	/// ID of the server that advertised this capability.
	pub server_id: Uuid,
	/// The backup type advertised (e.g. `tamanu-postgres`).
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Whether this `(server, type)` pair is enabled — only enabled pairs are
	/// scheduled and issued credentials.
	pub enabled: bool,
	/// When the server first advertised this capability.
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
	/// the group — i.e. the types the group is actively expected to back up on a
	/// schedule. Used for scheduling cadence and staleness alerting.
	pub async fn enabled_types_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Vec<BackupType>> {
		Self::types_for_group(db, group_id, true).await
	}

	/// Distinct backup types **declared** (advertised by bestool) on any
	/// non-archived server in the group, regardless of their enabled flag — i.e.
	/// every type the repo can hold snapshots for, including manual-only
	/// (disabled) ones. Retention is resolved per declared type so a manual
	/// backup of a non-scheduled type still gets its own policy, not the global.
	pub async fn declared_types_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Vec<BackupType>> {
		Self::types_for_group(db, group_id, false).await
	}

	async fn types_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		enabled_only: bool,
	) -> Result<Vec<BackupType>> {
		use crate::schema::{server_backup_capabilities as cap, servers};

		let mut q = cap::table
			.inner_join(servers::table.on(servers::id.eq(cap::server_id)))
			.filter(servers::group_id.eq(group_id))
			.filter(servers::deleted_at.is_null())
			.into_boxed();
		if enabled_only {
			q = q.filter(cap::enabled.eq(true));
		}
		q.select(cap::type_)
			.distinct()
			.load::<BackupType>(db)
			.await
			.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// server_group_backup_schedule — per-(group,type) schedule/retention overrides
// ---------------------------------------------------------------------------

/// A group's override of the schedule and/or retention for one backup type,
/// taking precedence over the canopy-wide default for that type.
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::server_group_backup_schedule)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroupBackupSchedule {
	/// ID of the server group this override applies to.
	pub group_id: Uuid,
	/// The backup type this override applies to (e.g. `tamanu-postgres`).
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Interval, in seconds, between scheduled backups of this type for this
	/// group. `None` means manual-only (no schedule) — distinct from a
	/// present value of 0.
	#[schema(value_type = Option<i64>, format = "int64")]
	pub expected_interval: Option<PgDuration>,
	/// Retention policy override for this group and type, in the same shape
	/// as the retention policy schema. `None` means inherit the type's
	/// canopy-wide default.
	pub retention: Option<JsonValue>,
	/// When this override was first created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// When this override was last updated.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// Opt out of the org retention floor for this override (dangerous): the
	/// floor is neither validated on write nor enforced on resolve.
	pub allow_below_floor: bool,
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
	pub allow_below_floor: bool,
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
				dsl::allow_below_floor.eq(new.allow_below_floor),
				dsl::updated_at.eq(now),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Remove a group's per-`(group,type)` schedule override so the type reverts
	/// to inheriting the canopy-wide default. No-op if there was no override.
	pub async fn delete(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		r#type: &BackupType,
	) -> Result<()> {
		use crate::schema::server_group_backup_schedule::dsl;

		diesel::delete(
			dsl::server_group_backup_schedule
				.filter(dsl::group_id.eq(group_id))
				.filter(dsl::type_.eq(r#type.as_str())),
		)
		.execute(db)
		.await
		.map_err(AppError::from)?;
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// backup_credential_issuances — audit log of every STS issuance
// ---------------------------------------------------------------------------

/// Audit record of one set of temporary S3 credentials issued to a device for
/// backup or restore. Only the access key id is recorded — the secret key and
/// session token are handed to the device but never stored.
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::backup_credential_issuances)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupCredentialIssuance {
	/// Unique id of this issuance record.
	pub id: i64,
	/// ID of the device the credentials were issued to.
	pub device_id: Uuid,
	/// ID of the server group whose backup repository the credentials grant
	/// access to.
	pub group_id: Uuid,
	/// The backup type the credentials were issued for (e.g.
	/// `tamanu-postgres`).
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// When the credentials were issued.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub issued_at: Timestamp,
	/// When the credentials expire and are no longer usable.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub expires_at: Timestamp,
	/// Whether these credentials were issued for uploading a backup or for
	/// restoring from one.
	pub purpose: BackupPurpose,
	/// ARN of the IAM role that was assumed to produce these credentials.
	pub sts_assumed_role: String,
	/// Request id AWS STS returned for the AssumeRole call, if available.
	pub sts_request_id: Option<String>,
	/// Access key id of the issued credentials. The corresponding secret key
	/// and session token are never persisted.
	pub access_key_id: Option<String>,
	/// Name of the S3 bucket these credentials grant access to, as it was at
	/// the time of issuance.
	pub bucket: String,
	/// Key prefix within the bucket these credentials grant access to, as it
	/// was at the time of issuance.
	pub prefix: String,
	/// The run this issuance was minted for, when the client supplied its
	/// run-uuid on the credential request. Ties an issuance to its reported run
	/// exactly (so duration and same-server concurrent runs are unambiguous).
	/// `None` for older clients that don't send it, which fall back to
	/// time-window matching.
	pub run_id: Option<Uuid>,
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
	/// Optional run-uuid the client minted at run start, correlating this
	/// issuance with the run it belongs to.
	pub run_id: Option<Uuid>,
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

	/// Issuances for a group issued at or after `since`, newest-first. Backs the
	/// recent-runs view: each issuance is a run *start*, paired with a reported run
	/// (to derive its duration) or surfaced on its own (an unreported / in-flight
	/// restore or backup).
	pub async fn list_for_group_since(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		since: Timestamp,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_credential_issuances::dsl;

		dsl::backup_credential_issuances
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::issued_at.ge(jiff_diesel::Timestamp::from(since)))
			.order(dsl::issued_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Latest *backup-purpose* credential issuance per `(device, type)` within a
	/// group, as `(issued_at, expires_at)`. Keyed `(device_id, type)`; restore
	/// issuances are excluded. Used to infer an in-flight backup: creds still
	/// within their validity window with no newer run report.
	pub async fn latest_backup_by_device_type_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<HashMap<(Uuid, BackupType), (Timestamp, Timestamp)>> {
		use crate::schema::backup_credential_issuances::dsl;

		let rows: Vec<Self> = dsl::backup_credential_issuances
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::purpose.eq(BackupPurpose::Backup))
			.distinct_on((dsl::device_id, dsl::type_))
			.order_by((dsl::device_id, dsl::type_, dsl::issued_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)?;

		Ok(rows
			.into_iter()
			.map(|r| ((r.device_id, r.r#type.clone()), (r.issued_at, r.expires_at)))
			.collect())
	}
}

// ---------------------------------------------------------------------------
// backup_runs — what bestool reported per run (client-minted UUID PK)
// ---------------------------------------------------------------------------

/// A backup or restore run reported by a device, with its outcome and any
/// size/traffic figures collected for it.
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::backup_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRun {
	/// Unique id of this run, minted by the reporting device.
	pub id: Uuid,
	/// ID of the device that performed and reported this run.
	pub device_id: Uuid,
	/// ID of the server group the run's backup repository belongs to.
	pub group_id: Uuid,
	/// ID of the server the run was performed for, if known.
	pub server_id: Option<Uuid>,
	/// The backup type this run performed (e.g. `tamanu-postgres`).
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Whether this run was a backup (upload) or a restore (download).
	pub purpose: BackupPurpose,
	/// Whether the run succeeded or failed.
	pub outcome: RunOutcome,
	/// Error message reported for a failed run, if any.
	pub error: Option<String>,
	/// Number of bytes the device reports having uploaded for this run.
	pub bytes_uploaded: Option<i64>,
	/// Id of the snapshot this run produced, if any.
	pub snapshot_id: Option<String>,
	/// When this run was reported.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub reported_at: Timestamp,
	/// Bytes sent to S3 for this run, counting the full HTTP request including
	/// signing/chunking overhead.
	pub s3_sent_raw_bytes: Option<i64>,
	/// Bytes sent to S3 for this run, counting only the decoded object data
	/// (excludes request/signing overhead).
	pub s3_sent_payload_bytes: Option<i64>,
	/// Bytes received from S3 for this run, counting the full HTTP response
	/// including framing overhead.
	pub s3_received_raw_bytes: Option<i64>,
	/// Bytes received from S3 for this run, counting only the decoded object
	/// data (excludes response framing overhead).
	pub s3_received_payload_bytes: Option<i64>,
	/// Logical (uncompressed) size of this run's snapshot, as independently
	/// observed by repository inspection rather than reported by the device.
	/// Distinct from `bytes_uploaded`; filled in after the fact, once, and
	/// never overwritten.
	pub snapshot_logical_bytes: Option<i64>,
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
	pub s3_sent_raw_bytes: Option<i64>,
	pub s3_sent_payload_bytes: Option<i64>,
	pub s3_received_raw_bytes: Option<i64>,
	pub s3_received_payload_bytes: Option<i64>,
}

/// Multi-field filter for the fleet-wide backup-run history query. Each field
/// is opt-in: `Default` matches every run.
#[derive(Debug, Clone, Default)]
pub struct BackupRunFilters {
	pub group_id: Option<Uuid>,
	pub server_id: Option<Uuid>,
	pub r#type: Option<BackupType>,
	pub outcome: Option<RunOutcome>,
	/// When `Some`, restrict to runs reported at or after this time.
	pub since: Option<Timestamp>,
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

	/// Latest *reported* backup per `(server, type)` within a group, regardless of
	/// outcome (success or failure). Keyed `(server_id, type)`. Used to tell
	/// whether a backup has been reported since credentials were last issued —
	/// i.e. whether one is still in flight.
	pub async fn latest_report_by_server_type_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<HashMap<(Uuid, BackupType), Timestamp>> {
		use crate::schema::backup_runs::dsl;

		let rows: Vec<Self> = dsl::backup_runs
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::purpose.eq(BackupPurpose::Backup))
			.filter(dsl::server_id.is_not_null())
			.distinct_on((dsl::server_id, dsl::type_))
			.order_by((dsl::server_id, dsl::type_, dsl::reported_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)?;

		Ok(rows
			.into_iter()
			.filter_map(|r| {
				r.server_id
					.map(|sid| ((sid, r.r#type.clone()), r.reported_at))
			})
			.collect())
	}

	/// When the group last had a successful *backup* reported (any server/type),
	/// or `None` if it never has. Drives prompt post-backup inspection.
	pub async fn latest_backup_at_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Option<Timestamp>> {
		use crate::schema::backup_runs::dsl;

		let row: Option<Self> = dsl::backup_runs
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::purpose.eq(BackupPurpose::Backup))
			.filter(dsl::outcome.eq(RunOutcome::Success))
			.order_by(dsl::reported_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?;
		Ok(row.map(|r| r.reported_at))
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

	/// Fleet-wide run history, newest first, narrowed by whichever filters are
	/// set. Backs the MCP `list_backup_runs` tool.
	pub async fn list_filtered(
		db: &mut AsyncPgConnection,
		filters: BackupRunFilters,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_runs::dsl;

		let mut q = dsl::backup_runs.into_boxed();
		if let Some(gid) = filters.group_id {
			q = q.filter(dsl::group_id.eq(gid));
		}
		if let Some(sid) = filters.server_id {
			q = q.filter(dsl::server_id.eq(sid));
		}
		if let Some(ty) = &filters.r#type {
			q = q.filter(dsl::type_.eq(ty.as_str()));
		}
		if let Some(outcome) = filters.outcome {
			q = q.filter(dsl::outcome.eq(outcome));
		}
		if let Some(since) = filters.since {
			q = q.filter(dsl::reported_at.ge(jiff_diesel::Timestamp::from(since)));
		}
		q.order(dsl::reported_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Total S3 bytes sent/received (raw — the full wire size including SigV4
	/// chunk framing, not the decoded payload) across the group's device backup
	/// runs reported so far this calendar month (UTC boundary). Returns
	/// `(sent, received)`, `0` for a side with no tallied runs.
	///
	/// This only covers what bestool's proxy tallies on `backup_runs` during
	/// device backups — repo maintenance and inspection traffic against the
	/// bucket isn't tallied anywhere, so the total undercounts the bucket's
	/// actual monthly S3 traffic.
	pub async fn s3_traffic_this_month_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<(i64, i64)> {
		#[derive(diesel::QueryableByName)]
		struct Totals {
			#[diesel(sql_type = diesel::sql_types::Int8)]
			sent: i64,
			#[diesel(sql_type = diesel::sql_types::Int8)]
			received: i64,
		}

		// SUM() over bigint columns promotes to numeric in Postgres, so the
		// query casts back to bigint explicitly; COALESCE turns "no rows"/
		// "all NULL" into 0 rather than surfacing NULL.
		let totals: Totals = diesel::sql_query(
			"SELECT \
				COALESCE(SUM(s3_sent_raw_bytes), 0)::bigint AS sent, \
				COALESCE(SUM(s3_received_raw_bytes), 0)::bigint AS received \
			FROM backup_runs \
			WHERE group_id = $1 \
				AND reported_at >= date_trunc('month', now(), 'UTC')",
		)
		.bind::<diesel::sql_types::Uuid, _>(group_id)
		.get_result(db)
		.await
		.map_err(AppError::from)?;

		Ok((totals.sent, totals.received))
	}

	/// The size of each named snapshot, resolved from the *backup* runs that
	/// produced them: the device-reported size (`bytes_uploaded`) or, failing
	/// that, the inspection-observed size (`snapshot_logical_bytes`) — the same
	/// preference the recent-runs UI applies to the producing run itself. Keyed
	/// by snapshot id; ids with no sized producing run are absent.
	///
	/// This is what lets a restore run show the size of the snapshot it used
	/// immediately: the snapshot was sized when the backup that created it ran
	/// (or was inspected), so there is no need to wait for inspection to
	/// backfill the restore run's own row.
	pub async fn snapshot_sizes_by_id(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		snapshot_ids: &[String],
	) -> Result<HashMap<String, i64>> {
		use crate::schema::backup_runs::dsl;

		if snapshot_ids.is_empty() {
			return Ok(HashMap::new());
		}

		let rows: Vec<Self> = dsl::backup_runs
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::purpose.eq(BackupPurpose::Backup))
			.filter(dsl::snapshot_id.eq_any(snapshot_ids))
			.filter(
				dsl::bytes_uploaded
					.is_not_null()
					.or(dsl::snapshot_logical_bytes.is_not_null()),
			)
			.distinct_on(dsl::snapshot_id)
			.order_by((dsl::snapshot_id, dsl::reported_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)?;

		Ok(rows
			.into_iter()
			.filter_map(|r| {
				let id = r.snapshot_id?;
				let size = r.bytes_uploaded.or(r.snapshot_logical_bytes)?;
				Some((id, size))
			})
			.collect())
	}

	/// Fill `snapshot_logical_bytes` from repo inspection for runs matched by
	/// snapshot id, only where it is still unset — write-once, since a snapshot
	/// is immutable. `sizes` maps a snapshot id to its observed logical size.
	/// Returns the number of runs updated.
	pub async fn backfill_snapshot_logical_bytes(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
		sizes: &[(String, i64)],
	) -> Result<usize> {
		use crate::schema::backup_runs::dsl;

		let mut updated = 0usize;
		for (snapshot_id, size) in sizes {
			updated += diesel::update(dsl::backup_runs)
				.filter(dsl::group_id.eq(group_id))
				.filter(dsl::snapshot_id.eq(snapshot_id))
				.filter(dsl::snapshot_logical_bytes.is_null())
				.set(dsl::snapshot_logical_bytes.eq(size))
				.execute(db)
				.await
				.map_err(AppError::from)?;
		}
		Ok(updated)
	}

	/// The latest backup run per `(server, type)` in a group that carries both a
	/// device-reported size (`bytes_uploaded`) and an inspection-observed size
	/// (`snapshot_logical_bytes`), both non-zero. Keyed `(server_id, type)` →
	/// `(reported, observed)`. The basis for the size-discrepancy check.
	pub async fn latest_sized_by_server_type_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<HashMap<(Uuid, BackupType), (i64, i64)>> {
		use crate::schema::backup_runs::dsl;

		let rows: Vec<Self> = dsl::backup_runs
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::purpose.eq(BackupPurpose::Backup))
			.filter(dsl::server_id.is_not_null())
			.filter(dsl::bytes_uploaded.is_not_null())
			.filter(dsl::snapshot_logical_bytes.is_not_null())
			.distinct_on((dsl::server_id, dsl::type_))
			.order_by((dsl::server_id, dsl::type_, dsl::reported_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)?;

		Ok(rows
			.into_iter()
			.filter_map(|r| {
				let sid = r.server_id?;
				let (up, snap) = (r.bytes_uploaded?, r.snapshot_logical_bytes?);
				(up > 0 && snap > 0).then_some(((sid, r.r#type.clone()), (up, snap)))
			})
			.collect())
	}
}

// ---------------------------------------------------------------------------
// backup_maintenance_runs — Canopy-owned maintenance outcomes (per-group)
// ---------------------------------------------------------------------------

/// One run of repository maintenance (compaction, garbage collection, etc.)
/// for a group's backup repository.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_maintenance_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupMaintenanceRun {
	/// Unique id of this maintenance run.
	pub id: i64,
	/// ID of the server group whose backup repository was maintained.
	pub group_id: Uuid,
	/// Which maintenance cycle this run performed: a lightweight "quick" pass
	/// or a more thorough "full" pass.
	pub kind: MaintenanceKind,
	/// When this maintenance run started.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub started_at: Timestamp,
	/// When this maintenance run finished. `None` while still running.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub finished_at: Option<Timestamp>,
	/// Outcome of the run. `None` while still running.
	pub outcome: Option<RunOutcome>,
	/// Error message for a failed run, if any.
	pub error: Option<String>,
	/// Number of bytes of unreferenced data reclaimed by this run, if known.
	pub bytes_reclaimed: Option<i64>,
}

/// Filter value for [`BackupMaintenanceRunFilters::outcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceOutcomeFilter {
	/// Still running (`outcome IS NULL`).
	Running,
	Outcome(RunOutcome),
}

/// Multi-field filter for the fleet-wide maintenance-run history query. Each
/// field is opt-in: `Default` matches every run.
#[derive(Debug, Clone, Default)]
pub struct BackupMaintenanceRunFilters {
	pub group_id: Option<Uuid>,
	pub kind: Option<MaintenanceKind>,
	pub outcome: Option<MaintenanceOutcomeFilter>,
	/// When `Some`, restrict to runs started at or after this time.
	pub since: Option<Timestamp>,
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

	/// Fleet-wide maintenance-run history, newest first, narrowed by whichever
	/// filters are set. Backs the MCP `list_maintenance_runs` tool.
	pub async fn list_filtered(
		db: &mut AsyncPgConnection,
		filters: BackupMaintenanceRunFilters,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::backup_maintenance_runs::dsl;

		let mut q = dsl::backup_maintenance_runs.into_boxed();
		if let Some(gid) = filters.group_id {
			q = q.filter(dsl::group_id.eq(gid));
		}
		if let Some(kind) = filters.kind {
			q = q.filter(dsl::kind.eq(kind));
		}
		match filters.outcome {
			Some(MaintenanceOutcomeFilter::Running) => {
				q = q.filter(dsl::outcome.is_null());
			}
			Some(MaintenanceOutcomeFilter::Outcome(o)) => {
				q = q.filter(dsl::outcome.eq(Some(o)));
			}
			None => {}
		}
		if let Some(since) = filters.since {
			q = q.filter(dsl::started_at.ge(jiff_diesel::Timestamp::from(since)));
		}
		q.order(dsl::started_at.desc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The most recently *finished* maintenance run for the group (any
	/// outcome), ignoring runs still in flight (`outcome IS NULL`). Used by the
	/// detection sweep to decide whether the latest concluded run failed.
	pub async fn latest_completed_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Option<Self>> {
		use crate::schema::backup_maintenance_runs::dsl;

		dsl::backup_maintenance_runs
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::outcome.is_not_null())
			.order(dsl::finished_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// When the group's maintenance last completed *successfully*, or `None` if
	/// it never has. Drives prompt post-maintenance inspection: maintenance
	/// (retention pruning, compaction) changes what's actually in the repo, so
	/// the stats/snapshot inventory should freshen soon after — mirroring
	/// [`BackupRun::latest_backup_at_for_group`]'s "freshen after a backup"
	/// role. A failed run doesn't change repo contents and is already surfaced
	/// via the separate `MAINTENANCE_ERROR` alert, so only successes count
	/// here (matching the success-only convention there).
	pub async fn latest_successful_finished_at_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Option<Timestamp>> {
		use crate::schema::backup_maintenance_runs::dsl;

		let row: Option<Self> = dsl::backup_maintenance_runs
			.filter(dsl::group_id.eq(group_id))
			.filter(dsl::outcome.eq(RunOutcome::Success))
			.order_by(dsl::finished_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?;
		Ok(row.and_then(|r| r.finished_at))
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

/// Inventory of one backup source found in a group's repository during
/// inspection, independent of what canopy's own run records say. This is the
/// ground-truth view of what's actually stored, used to reconcile against
/// reported runs.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_repo_snapshots)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRepoSnapshot {
	/// ID of the server group this repository belongs to.
	pub group_id: Uuid,
	/// Raw source identifier as recorded in the backup repository (typically
	/// combines a host and path). Used to match this inventory row back to a
	/// server and backup type.
	pub source: String,
	/// Server this source was matched to, if the source identifier could be
	/// parsed as one of the group's servers.
	pub server_id: Option<Uuid>,
	/// Backup type this source was matched to, if the source identifier could
	/// be parsed as a known backup type.
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = Option<String>)]
	pub r#type: Option<BackupType>,
	/// Timestamp of the most recent snapshot found for this source in the
	/// repository, if any.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub latest_snapshot_at: Option<Timestamp>,
	/// When this source's inventory was last refreshed by inspection.
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

	/// When the group's repo was last inspected (newest `observed_at` across its
	/// sources), or `None` if never. This table is written *only* by the
	/// inspection Job, so it's the clean "last inspected" signal.
	pub async fn last_inspected_at_for_group(
		db: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Option<Timestamp>> {
		use crate::schema::backup_repo_snapshots::dsl;

		let row: Option<Self> = dsl::backup_repo_snapshots
			.filter(dsl::group_id.eq(group_id))
			.order_by(dsl::observed_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?;
		Ok(row.map(|r| r.observed_at))
	}
}

// ---------------------------------------------------------------------------
// backup_repo_stats — cached repo + bucket stats (per-group, two writers)
// ---------------------------------------------------------------------------

/// Cached size and count statistics for a group's backup repository and its
/// underlying bucket. Populated by two independent processes: repository
/// inspection fills the snapshot/source/logical/physical figures, and a
/// separate bucket-metrics collector fills `bucket_bytes`; either can be
/// stale relative to the other.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_repo_stats)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRepoStats {
	/// ID of the server group these stats describe.
	pub group_id: Uuid,
	/// Total number of snapshots currently in the repository, if known.
	pub snapshot_count: Option<i32>,
	/// Number of distinct backup sources (servers/types) currently in the
	/// repository, if known.
	pub source_count: Option<i32>,
	/// Total logical (uncompressed, pre-dedup) size of all data in the
	/// repository, in bytes, if known.
	pub logical_bytes: Option<i64>,
	/// Total physical (as actually stored, after compression/dedup) size of
	/// the repository, in bytes, if known.
	pub physical_bytes: Option<i64>,
	/// Total size of the underlying S3 bucket, in bytes, as reported by cloud
	/// storage metrics. Can differ from `physical_bytes` (e.g. it includes
	/// content outside the repository, or reflects a different point in
	/// time).
	pub bucket_bytes: Option<i64>,
	/// When the repo-derived figures (snapshot/source counts, logical/physical
	/// bytes) were last refreshed by the inspection job.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub observed_at: Timestamp,
	/// When `bucket_bytes` was last refreshed by the (daily) S3-metrics
	/// collector, if ever. Tracked separately from `observed_at` because the
	/// two figures are collected on independent cadences.
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub bucket_bytes_observed_at: Option<Timestamp>,
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
	/// (+ `bucket_bytes_observed_at`), never the repo-derived fields or their
	/// `observed_at`.
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
				dsl::bucket_bytes_observed_at.eq(now),
			))
			.on_conflict(dsl::group_id)
			.do_update()
			.set((
				dsl::bucket_bytes.eq(bucket_bytes),
				dsl::bucket_bytes_observed_at.eq(now),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// backup_requests — pending operator one-off "backup now" flags
// ---------------------------------------------------------------------------

/// A pending one-off "run now" request for a server, outside its normal
/// schedule. Cleared once the requested run is reported.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::backup_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRequest {
	/// ID of the server the one-off run is requested for.
	pub server_id: Uuid,
	/// The backup type to run (e.g. `tamanu-postgres`).
	#[diesel(column_name = type_)]
	#[serde(rename = "type")]
	#[schema(value_type = String)]
	pub r#type: BackupType,
	/// Whether the requested run is a backup or a restore.
	pub purpose: BackupPurpose,
	/// When this request was made (or last refreshed, if re-requested).
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub requested_at: Timestamp,
	/// Who made the request, if known.
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

	/// Whether a one-off request is pending for `(server, type, purpose)`.
	pub async fn exists(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		r#type: &BackupType,
		purpose: BackupPurpose,
	) -> Result<bool> {
		use crate::schema::backup_requests::dsl;
		use diesel::dsl::{exists, select};

		select(exists(
			dsl::backup_requests
				.filter(dsl::server_id.eq(server_id))
				.filter(dsl::type_.eq(r#type.as_str()))
				.filter(dsl::purpose.eq(purpose)),
		))
		.get_result(db)
		.await
		.map_err(AppError::from)
	}
}

// ---------------------------------------------------------------------------
// backup_recovery_verifications — the recovery vault verification-ceremony log
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Queryable, Selectable)]
#[diesel(table_name = crate::schema::backup_recovery_verifications)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BackupRecoveryVerification {
	pub id: i64,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub verified_at: Timestamp,
	/// The recipient fingerprints (age1… keys) this ceremony covered, as a JSON
	/// array of strings.
	pub recipients: JsonValue,
}

impl BackupRecoveryVerification {
	/// Record a successful ceremony against the given recipient fingerprints.
	pub async fn record(db: &mut AsyncPgConnection, recipients: &[String]) -> Result<Self> {
		use crate::schema::backup_recovery_verifications::dsl;

		let recipients =
			serde_json::to_value(recipients).unwrap_or_else(|_| JsonValue::Array(vec![]));
		diesel::insert_into(dsl::backup_recovery_verifications)
			.values(dsl::recipients.eq(recipients))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// The most recent verification, if any.
	pub async fn latest(db: &mut AsyncPgConnection) -> Result<Option<Self>> {
		use crate::schema::backup_recovery_verifications::dsl;

		dsl::backup_recovery_verifications
			.order(dsl::verified_at.desc())
			.select(Self::as_select())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// The recipient fingerprints this verification covered.
	pub fn recipient_list(&self) -> Vec<String> {
		serde_json::from_value(self.recipients.clone()).unwrap_or_default()
	}
}
