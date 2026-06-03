//! Proof-of-possession challenges. `begin` issues a short-lived random nonce
//! bound to the presented public key and the token it is for; `complete`
//! verifies a signature over the nonce against that key, then takes the
//! challenge (single-use) and consumes the token. The nonce is one-shot and not
//! a reusable secret, so it is stored and compared as-is.

use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

#[derive(Clone, Debug, Queryable, Selectable)]
#[diesel(table_name = crate::schema::server_enrollment_challenges)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerEnrollmentChallenge {
	pub id: Uuid,
	pub server_id: Uuid,
	pub token_hash: Vec<u8>,
	pub public_key: Vec<u8>,
	pub nonce: Vec<u8>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub expires_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp)]
	pub used_at: Option<Timestamp>,
}

impl ServerEnrollmentChallenge {
	/// Issue a fresh nonce bound to (server, token, presented key). Returns the
	/// raw nonce bytes for the caller to hand back to the device.
	pub async fn create(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		token_hash: &[u8],
		public_key: &[u8],
		ttl: SignedDuration,
	) -> Result<Vec<u8>> {
		use crate::schema::server_enrollment_challenges::dsl;

		let mut nonce = [0u8; 32];
		getrandom::fill(&mut nonce)
			.map_err(|e| AppError::custom(format!("CSPRNG failure: {e}")))?;
		let expires_at = Timestamp::now()
			.checked_add(ttl)
			.map_err(|e| AppError::custom(format!("bad challenge ttl: {e}")))?;

		diesel::insert_into(dsl::server_enrollment_challenges)
			.values((
				dsl::server_id.eq(server_id),
				dsl::token_hash.eq(token_hash),
				dsl::public_key.eq(public_key),
				dsl::nonce.eq(&nonce[..]),
				dsl::expires_at.eq(jiff_diesel::Timestamp::from(expires_at)),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;

		Ok(nonce.to_vec())
	}

	/// Single-use take: atomically mark the challenge used iff it is still valid
	/// for (server, nonce, key). Returns the row (carrying `token_hash`) so the
	/// caller can consume the matching token. Errors generically otherwise.
	pub async fn take(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		nonce: &[u8],
		public_key: &[u8],
	) -> Result<Self> {
		use crate::schema::server_enrollment_challenges::dsl;

		diesel::update(
			dsl::server_enrollment_challenges
				.filter(dsl::server_id.eq(server_id))
				.filter(dsl::nonce.eq(nonce))
				.filter(dsl::public_key.eq(public_key))
				.filter(dsl::used_at.is_null())
				.filter(dsl::expires_at.gt(diesel::dsl::now)),
		)
		.set(dsl::used_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))))
		.returning(Self::as_select())
		.get_result(db)
		.await
		.optional()
		.map_err(AppError::from)?
		.ok_or(AppError::EnrollmentFailed)
	}
}
