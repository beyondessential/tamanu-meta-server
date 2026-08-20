//! Tokens embedded in the URL of the planned-upgrades calendar feed.
//!
//! Spec: `.workhorse/specs/private-server/upgrade-plans.md` (id `UPG`), "The
//! calendar feed".
//!
//! The feed URL is handed to the operator once at minting; we persist only the
//! SHA-256 hash of the token in it. A token is usable while `revoked_at IS
//! NULL`: a calendar a subscriber never opens again would lapse silently, so a
//! feed ends by being revoked rather than by running out.

use base64::Engine;
use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Recognizable prefix on the plaintext so a leaked feed URL is identifiable in
/// secret scanning, and so operators can tell what a stray credential is for.
pub const TOKEN_PREFIX: &str = "canopy_cal_";

#[derive(Clone, Debug, Queryable, Selectable)]
#[diesel(table_name = crate::schema::calendar_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct CalendarToken {
	pub id: Uuid,
	pub name: String,
	pub token_hash: Vec<u8>,
	pub created_by: String,
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp)]
	pub revoked_at: Option<Timestamp>,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp)]
	pub last_used_at: Option<Timestamp>,
}

/// SHA-256 of the token string. Unsalted is correct here: the token is 256 bits
/// of CSPRNG output, so there is no dictionary/brute-force risk — do not
/// "upgrade" this to HMAC/argon. The whole-digest equality lives in the SQL
/// `WHERE`, never an in-memory plaintext compare.
fn hash_token(plaintext: &str) -> Vec<u8> {
	Sha256::digest(plaintext.as_bytes()).to_vec()
}

impl CalendarToken {
	/// Mint a fresh token, returning the row and the plaintext (which the
	/// caller must show once and never persist or log).
	pub async fn mint(
		db: &mut AsyncPgConnection,
		name: &str,
		created_by: &str,
	) -> Result<(Self, String)> {
		use crate::schema::calendar_tokens::dsl;

		let mut raw = [0u8; 32];
		getrandom::fill(&mut raw).map_err(|e| AppError::custom(format!("CSPRNG failure: {e}")))?;
		let plaintext = format!(
			"{TOKEN_PREFIX}{}",
			base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
		);

		let token = diesel::insert_into(dsl::calendar_tokens)
			.values((
				dsl::name.eq(name),
				dsl::token_hash.eq(hash_token(&plaintext)),
				dsl::created_by.eq(created_by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)?;

		Ok((token, plaintext))
	}

	/// Look up a usable (un-revoked) token by its plaintext. `None` for unknown
	/// and revoked alike — the caller must not distinguish those to the
	/// requester.
	pub async fn find_active(db: &mut AsyncPgConnection, plaintext: &str) -> Result<Option<Self>> {
		use crate::schema::calendar_tokens::dsl;

		dsl::calendar_tokens
			.select(Self::as_select())
			.filter(dsl::token_hash.eq(hash_token(plaintext)))
			.filter(dsl::revoked_at.is_null())
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Record use of a token. Throttled: skips the write when `last_used_at` is
	/// under a minute old, so a calendar client that polls hard costs one
	/// UPDATE per minute.
	pub async fn touch_last_used(db: &mut AsyncPgConnection, id: Uuid) -> Result<()> {
		use crate::schema::calendar_tokens::dsl;

		let cutoff = Timestamp::now()
			.checked_sub(SignedDuration::from_secs(60))
			.map_err(|e| AppError::custom(format!("bad throttle window: {e}")))?;
		diesel::update(
			dsl::calendar_tokens.filter(dsl::id.eq(id)).filter(
				dsl::last_used_at
					.is_null()
					.or(dsl::last_used_at.lt(jiff_diesel::Timestamp::from(cutoff))),
			),
		)
		.set(dsl::last_used_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))))
		.execute(db)
		.await
		.map_err(AppError::from)?;
		Ok(())
	}

	/// All tokens, newest first, revoked ones included (the UI shows history).
	pub async fn list(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::calendar_tokens::dsl;

		dsl::calendar_tokens
			.select(Self::as_select())
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Revoke a token, effective immediately; idempotent on an already-revoked
	/// token. Errors (404) on an unknown id.
	pub async fn revoke(db: &mut AsyncPgConnection, id: Uuid) -> Result<()> {
		use crate::schema::calendar_tokens::dsl;

		let affected = diesel::update(
			dsl::calendar_tokens
				.filter(dsl::id.eq(id))
				.filter(dsl::revoked_at.is_null()),
		)
		.set(dsl::revoked_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))))
		.execute(db)
		.await
		.map_err(AppError::from)?;

		if affected == 0 {
			let exists: i64 = dsl::calendar_tokens
				.filter(dsl::id.eq(id))
				.count()
				.get_result(db)
				.await
				.map_err(AppError::from)?;
			if exists == 0 {
				return Err(diesel::result::Error::NotFound.into());
			}
		}
		Ok(())
	}
}
