//! Certificate orders and the certificates they produce (CRT).
//!
//! One row per (name, certified key). A repeat request for a key Canopy already
//! holds a certificate for is answered from the row rather than ordering again,
//! which is what keeps a server that lost its local copy from spending the
//! authority's budget; a request naming a different key is a different row and a
//! new order.
//!
//! The submitted signing request is kept because renewal reuses it: the key has
//! not changed, so Canopy renews without needing anything from the server.
// spec: CRT#certificates

use commons_errors::{AppError, Result};
use commons_types::dns::normalize_domain;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use uuid::Uuid;

/// How long before expiry Canopy starts trying to renew. A third of a typical
/// ninety-day life, so there is room for many failed attempts before anything is
/// at risk.
pub const RENEW_BEFORE: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The longest Canopy waits between attempts at an order. Failures are usually
/// the authority being briefly unavailable or a record not yet visible, so the
/// interval grows — but not past this, or a certificate could expire while
/// Canopy waits.
pub const MAX_BACKOFF: SignedDuration = SignedDuration::from_hours(6);

/// The first interval between attempts, doubling from there.
const BASE_BACKOFF: SignedDuration = SignedDuration::from_secs(60);

/// Where an order stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderState {
	/// Waiting to be worked, or waiting to be retried after a failure.
	Pending,
	/// A certificate is held.
	Issued,
	/// Given up on for now; the reason is on the row.
	Failed,
}

impl OrderState {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Pending => "pending",
			Self::Issued => "issued",
			Self::Failed => "failed",
		}
	}

	pub fn from_column(raw: &str) -> Self {
		match raw {
			"issued" => Self::Issued,
			"failed" => Self::Failed,
			_ => Self::Pending,
		}
	}
}

/// A certificate Canopy holds, or an order in flight to obtain one.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_certificates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerCertificate {
	pub id: Uuid,
	/// The server the certificate was obtained for.
	pub server_id: Uuid,
	/// The single name it covers, normalised.
	pub name: String,
	/// Hex SHA-256 over the subject public key info of the certified key.
	pub key_fingerprint: String,
	/// The signing request as submitted, kept so renewal needs nothing from the
	/// server.
	#[serde(skip)]
	#[schema(value_type = String)]
	pub csr: Vec<u8>,
	/// `pending`, `issued`, or `failed`.
	pub state: String,
	/// The issued chain, PEM. Public material only.
	pub chain: Option<String>,
	/// When the certificate expires.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub not_after: Option<Timestamp>,
	/// When it was issued.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub issued_at: Option<Timestamp>,
	/// Whether the order in flight is extending a certificate that already
	/// issued, so a renewal failure is told apart from a first issuance that
	/// never came up.
	pub renewing: bool,
	pub attempts: i32,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub next_attempt_at: Timestamp,
	/// Why the last attempt failed, if it did.
	pub last_error: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

impl ServerCertificate {
	pub fn order_state(&self) -> OrderState {
		OrderState::from_column(&self.state)
	}

	/// Whether the held certificate is usable now: issued, and not expired.
	pub fn is_current(&self) -> bool {
		self.order_state() == OrderState::Issued
			&& self.chain.is_some()
			&& self.not_after.is_some_and(|at| at > Timestamp::now())
	}

	/// Ask for a certificate for `name` covering the key in `csr`.
	///
	/// Idempotent by (name, key): a request for a key Canopy already holds a
	/// current certificate for returns that certificate untouched, one already
	/// in flight returns the order as it stands, and one that had failed is
	/// picked up again from the start. Only a genuinely new (name, key) opens a
	/// new order.
	pub async fn request(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		name: &str,
		key_fingerprint: &str,
		csr: &[u8],
	) -> Result<Self> {
		use crate::schema::server_certificates::dsl;

		let name = normalize_domain(name)?;

		if let Some(existing) = Self::for_name_and_key(db, &name, key_fingerprint).await? {
			return match existing.order_state() {
				// Held and usable, or already being worked: nothing to do.
				OrderState::Issued if existing.is_current() => Ok(existing),
				OrderState::Pending => Ok(existing),
				// Expired, or given up on: try again now, from the start. The
				// server may have changed hands, so the requester is recorded
				// afresh.
				_ => diesel::update(dsl::server_certificates.filter(dsl::id.eq(existing.id)))
					.set((
						dsl::server_id.eq(server_id),
						dsl::state.eq(OrderState::Pending.as_str()),
						dsl::renewing.eq(existing.chain.is_some()),
						dsl::attempts.eq(0),
						dsl::next_attempt_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
						dsl::last_error.eq::<Option<String>>(None),
					))
					.returning(Self::as_select())
					.get_result(db)
					.await
					.map_err(AppError::from),
			};
		}

		diesel::insert_into(dsl::server_certificates)
			.values((
				dsl::server_id.eq(server_id),
				dsl::name.eq(&name),
				dsl::key_fingerprint.eq(key_fingerprint),
				dsl::csr.eq(csr),
				dsl::state.eq(OrderState::Pending.as_str()),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn for_name_and_key(
		db: &mut AsyncPgConnection,
		name: &str,
		key_fingerprint: &str,
	) -> Result<Option<Self>> {
		use crate::schema::server_certificates::dsl;
		let name = normalize_domain(name)?;
		dsl::server_certificates
			.select(Self::as_select())
			.filter(dsl::name.eq(name))
			.filter(dsl::key_fingerprint.eq(key_fingerprint))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	pub async fn get(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		use crate::schema::server_certificates::dsl;
		dsl::server_certificates
			.select(Self::as_select())
			.filter(dsl::id.eq(id))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)?
			.ok_or(AppError::DatabaseQuery(DieselError::NotFound))
	}

	/// Every certificate and in-flight order for a server, newest first.
	pub async fn for_server(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::server_certificates::dsl;
		dsl::server_certificates
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server_id))
			.order((dsl::name.asc(), dsl::created_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Orders due to be attempted, soonest first. Claimed with `SKIP LOCKED` so
	/// two workers never drive the same order.
	pub async fn claim_due(db: &mut AsyncPgConnection, limit: i64) -> Result<Vec<Self>> {
		use crate::schema::server_certificates::dsl;
		dsl::server_certificates
			.select(Self::as_select())
			.filter(dsl::state.eq(OrderState::Pending.as_str()))
			.filter(dsl::next_attempt_at.le(jiff_diesel::Timestamp::from(Timestamp::now())))
			.order(dsl::next_attempt_at.asc())
			.limit(limit)
			.for_update()
			.skip_locked()
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Record a certificate obtained, clearing the order.
	pub async fn record_issued(
		db: &mut AsyncPgConnection,
		id: Uuid,
		chain: &str,
		not_after: Timestamp,
	) -> Result<()> {
		use crate::schema::server_certificates::dsl;
		diesel::update(dsl::server_certificates.filter(dsl::id.eq(id)))
			.set((
				dsl::state.eq(OrderState::Issued.as_str()),
				dsl::chain.eq(Some(chain)),
				dsl::not_after.eq(jiff_diesel::NullableTimestamp::from(Some(not_after))),
				dsl::issued_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
				dsl::renewing.eq(false),
				dsl::attempts.eq(0),
				dsl::last_error.eq::<Option<String>>(None),
			))
			.execute(db)
			.await?;
		Ok(())
	}

	/// Record a failed attempt and when to try again.
	///
	/// The interval doubles per attempt up to [`MAX_BACKOFF`]. The order stays
	/// pending rather than becoming `failed`: an order Canopy has been asked for
	/// is worth continuing to try, and the alerting is what tells an operator it
	/// isn't working.
	pub async fn record_failure(db: &mut AsyncPgConnection, id: Uuid, error: &str) -> Result<()> {
		use crate::schema::server_certificates::dsl;

		let current = Self::get(db, id).await?;
		let attempts = current.attempts.saturating_add(1);
		let backoff = backoff_for(attempts);

		diesel::update(dsl::server_certificates.filter(dsl::id.eq(id)))
			.set((
				dsl::attempts.eq(attempts),
				dsl::next_attempt_at.eq(jiff_diesel::Timestamp::from(Timestamp::now() + backoff)),
				dsl::last_error.eq(Some(error)),
			))
			.execute(db)
			.await?;
		Ok(())
	}

	/// Certificates close enough to expiry to renew, and not already being
	/// worked. Marks each pending so the worker picks it up, and returns them.
	pub async fn start_renewals(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::server_certificates::dsl;

		let horizon = Timestamp::now() + RENEW_BEFORE;
		let due: Vec<Uuid> = dsl::server_certificates
			.select(dsl::id)
			.filter(dsl::state.eq(OrderState::Issued.as_str()))
			.filter(dsl::not_after.is_not_null())
			.filter(dsl::not_after.le(jiff_diesel::NullableTimestamp::from(Some(horizon))))
			.load(db)
			.await
			.map_err(AppError::from)?;

		if due.is_empty() {
			return Ok(Vec::new());
		}

		diesel::update(dsl::server_certificates.filter(dsl::id.eq_any(&due)))
			.set((
				dsl::state.eq(OrderState::Pending.as_str()),
				dsl::renewing.eq(true),
				dsl::attempts.eq(0),
				dsl::next_attempt_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
			))
			.execute(db)
			.await?;

		dsl::server_certificates
			.select(Self::as_select())
			.filter(dsl::id.eq_any(due))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Certificates expiring within `window` that Canopy has not managed to
	/// renew — what the expiry alert reports.
	pub async fn expiring_within(
		db: &mut AsyncPgConnection,
		window: SignedDuration,
	) -> Result<Vec<Self>> {
		use crate::schema::server_certificates::dsl;
		let horizon = Timestamp::now() + window;
		dsl::server_certificates
			.select(Self::as_select())
			.filter(dsl::not_after.is_not_null())
			.filter(dsl::not_after.le(jiff_diesel::NullableTimestamp::from(Some(horizon))))
			.order(dsl::not_after.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Orders that have never produced a certificate and have failed repeatedly
	/// — a deployment that never came up, as distinct from one about to go dark.
	pub async fn stuck_first_issuances(
		db: &mut AsyncPgConnection,
		min_attempts: i32,
	) -> Result<Vec<Self>> {
		use crate::schema::server_certificates::dsl;
		dsl::server_certificates
			.select(Self::as_select())
			.filter(dsl::chain.is_null())
			.filter(dsl::attempts.ge(min_attempts))
			.order(dsl::created_at.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Stop working an order and stop renewing what it produced — the name is no
	/// longer the group's, or the server may no longer hold it.
	pub async fn stop(db: &mut AsyncPgConnection, id: Uuid, reason: &str) -> Result<()> {
		use crate::schema::server_certificates::dsl;
		diesel::update(dsl::server_certificates.filter(dsl::id.eq(id)))
			.set((
				dsl::state.eq(OrderState::Failed.as_str()),
				dsl::last_error.eq(Some(reason)),
			))
			.execute(db)
			.await?;
		Ok(())
	}
}

/// Doubling backoff from [`BASE_BACKOFF`], capped at [`MAX_BACKOFF`].
pub fn backoff_for(attempts: i32) -> SignedDuration {
	let shift = attempts.clamp(1, 16) - 1;
	let seconds = BASE_BACKOFF.as_secs().saturating_mul(1i64 << shift.min(20));
	SignedDuration::from_secs(seconds.min(MAX_BACKOFF.as_secs()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn backoff_grows_then_settles() {
		assert_eq!(backoff_for(1), SignedDuration::from_secs(60));
		assert_eq!(backoff_for(2), SignedDuration::from_secs(120));
		assert_eq!(backoff_for(3), SignedDuration::from_secs(240));
		// Capped, and stays capped however many attempts have gone by.
		assert_eq!(backoff_for(20), MAX_BACKOFF);
		assert_eq!(backoff_for(i32::MAX), MAX_BACKOFF);
		// A zeroth attempt is treated as the first rather than as no wait.
		assert_eq!(backoff_for(0), SignedDuration::from_secs(60));
	}

	#[test]
	fn order_state_round_trips_through_the_column() {
		for state in [OrderState::Pending, OrderState::Issued, OrderState::Failed] {
			assert_eq!(OrderState::from_column(state.as_str()), state);
		}
		// Anything unrecognised is treated as work still to do rather than as
		// success.
		assert_eq!(
			OrderState::from_column("something else"),
			OrderState::Pending
		);
	}
}
