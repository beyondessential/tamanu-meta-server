//! Single-use enrollment tokens. The plaintext token travels only inside the
//! base64 blob handed to the operator; we persist only its SHA-256 hash. A
//! token is "active" while `consumed_at IS NULL AND expires_at > now()`. The
//! burn (setting `consumed_at`) happens atomically with a successful
//! enrollment; reissuing mints a new token and marks any prior un-consumed ones
//! consumed so exactly one is ever active.

use base64::Engine;
use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug, Queryable, Selectable)]
#[diesel(table_name = crate::schema::server_enrollment_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerEnrollmentToken {
	pub id: Uuid,
	pub server_id: Uuid,
	pub token_hash: Vec<u8>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub expires_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp)]
	pub consumed_at: Option<Timestamp>,
}

/// SHA-256 of the token string. Unsalted is correct here: the token is 256 bits
/// of CSPRNG output, so there is no dictionary/brute-force risk — do not
/// "upgrade" this to HMAC/argon. The whole-digest equality lives in the SQL
/// `WHERE`, never an in-memory plaintext compare.
fn hash_token(plaintext: &str) -> Vec<u8> {
	Sha256::digest(plaintext.as_bytes()).to_vec()
}

impl ServerEnrollmentToken {
	/// Mint a fresh token for a server, returning the row and the plaintext
	/// (which the caller must put in the blob and never persist or log). Any
	/// prior un-consumed tokens for the server are marked consumed in the same
	/// transaction.
	pub async fn mint(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		ttl: SignedDuration,
	) -> Result<(Self, String)> {
		use crate::schema::server_enrollment_tokens::dsl;

		let mut raw = [0u8; 32];
		getrandom::fill(&mut raw).map_err(|e| AppError::custom(format!("CSPRNG failure: {e}")))?;
		let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
		let token_hash = hash_token(&plaintext);
		let expires_at = Timestamp::now()
			.checked_add(ttl)
			.map_err(|e| AppError::custom(format!("bad token ttl: {e}")))?;

		let token = db
			.transaction::<_, AppError, _>(async |conn| {
				diesel::update(
					dsl::server_enrollment_tokens
						.filter(dsl::server_id.eq(server_id))
						.filter(dsl::consumed_at.is_null()),
				)
				.set(
					dsl::consumed_at
						.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
				)
				.execute(conn)
				.await
				.map_err(AppError::from)?;

				diesel::insert_into(dsl::server_enrollment_tokens)
					.values((
						dsl::server_id.eq(server_id),
						dsl::token_hash.eq(&token_hash),
						dsl::expires_at.eq(jiff_diesel::Timestamp::from(expires_at)),
					))
					.returning(Self::as_select())
					.get_result(conn)
					.await
					.map_err(AppError::from)
			})
			.await?;

		Ok((token, plaintext))
	}

	/// Validate (without consuming) that a plaintext token is active for a
	/// server. Returns the row (carrying `token_hash`) for the caller to bind a
	/// challenge to. Errors generically when not active.
	pub async fn find_active(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		plaintext: &str,
	) -> Result<Self> {
		use crate::schema::server_enrollment_tokens::dsl;

		dsl::server_enrollment_tokens
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server_id))
			.filter(dsl::token_hash.eq(hash_token(plaintext)))
			.filter(dsl::consumed_at.is_null())
			.filter(dsl::expires_at.gt(diesel::dsl::now))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?
			.ok_or(AppError::EnrollmentFailed)
	}

	/// Burn a token: atomically set `consumed_at` iff it is still active. Called
	/// inside the enrollment bind transaction. Errors if the token is no longer
	/// active (already consumed / expired / unknown), which the single `UPDATE`
	/// makes race-safe (concurrent completes: only one affects a row).
	pub async fn consume(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		token_hash: &[u8],
	) -> Result<()> {
		use crate::schema::server_enrollment_tokens::dsl;

		let affected = diesel::update(
			dsl::server_enrollment_tokens
				.filter(dsl::server_id.eq(server_id))
				.filter(dsl::token_hash.eq(token_hash))
				.filter(dsl::consumed_at.is_null())
				.filter(dsl::expires_at.gt(diesel::dsl::now)),
		)
		.set(dsl::consumed_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))))
		.execute(db)
		.await
		.map_err(AppError::from)?;

		if affected == 0 {
			return Err(AppError::EnrollmentFailed);
		}
		Ok(())
	}

	/// Revoke any outstanding (un-consumed) token for a server, e.g. an
	/// enrollment ticket issued by mistake. Marks them consumed so they can no
	/// longer be used; idempotent.
	pub async fn revoke(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
		use crate::schema::server_enrollment_tokens::dsl;
		diesel::update(
			dsl::server_enrollment_tokens
				.filter(dsl::server_id.eq(server_id))
				.filter(dsl::consumed_at.is_null()),
		)
		.set(dsl::consumed_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))))
		.execute(db)
		.await
		.map_err(AppError::from)?;
		Ok(())
	}

	/// The currently-active token for a server, if any. For the admin UI to show
	/// "expires <when>" — never reveals the secret (only the hash lives here).
	pub async fn active_for(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Option<Self>> {
		use crate::schema::server_enrollment_tokens::dsl;

		dsl::server_enrollment_tokens
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server_id))
			.filter(dsl::consumed_at.is_null())
			.filter(dsl::expires_at.gt(diesel::dsl::now))
			.order(dsl::created_at.desc())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}
}
