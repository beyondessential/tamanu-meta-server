//! Records of successful recovery vault writes.
//!
//! The backups pod encrypts and PUTs a fresh `state.age` snapshot to the vault
//! bucket every [`crate::backups`]-adjacent tick (see
//! `crates/jobs/src/backup/recovery_snapshot.rs`). The private server can't
//! cheaply check the vault bucket itself (the S3 config is jobs-pod-only env),
//! so the writer records each success here and the recovery vault settings
//! page reads it back.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use uuid::Uuid;

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = crate::schema::recovery_vault_writes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct RecoveryVaultWrite {
	pub id: Uuid,
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub written_at: Timestamp,
	pub bytes: i64,
}

impl RecoveryVaultWrite {
	/// Record a successful vault write of `bytes` ciphertext bytes.
	pub async fn record(db: &mut AsyncPgConnection, bytes: i64) -> Result<Self> {
		use crate::schema::recovery_vault_writes::dsl;

		diesel::insert_into(dsl::recovery_vault_writes)
			.values(dsl::bytes.eq(bytes))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// The most recent vault write, if any.
	pub async fn latest(db: &mut AsyncPgConnection) -> Result<Option<Self>> {
		use crate::schema::recovery_vault_writes::dsl;

		dsl::recovery_vault_writes
			.order(dsl::written_at.desc())
			.select(Self::as_select())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}
}
