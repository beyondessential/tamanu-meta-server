//! Doubling retry backoff.
//!
//! Several subsystems retry work that can fail for a while and then recover —
//! Slack deliveries, certificate issuance, kopia maintenance. They all want the
//! same shape (wait `base`, then twice that, then twice that again, held at a
//! `cap`) and only disagree on the two durations, so the arithmetic lives here
//! rather than being written out once per caller.
//!
//! What differs between callers is where the attempt count comes from, and that
//! stays with them: the DB-backed ones read a persisted `attempts` column, while
//! an in-process scheduler may count consecutive failures in memory.

use jiff::SignedDuration;

/// Beyond this the doubling is moot — every caller's cap is long since reached,
/// and `2^32` does not fit in the `i32` multiplier.
const MAX_DOUBLINGS: u32 = 30;

/// A doubling retry schedule between a `base` and a `cap` wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
	base: SignedDuration,
	cap: SignedDuration,
}

impl Backoff {
	/// A schedule starting at `base` and held at `cap`.
	///
	/// A `cap` below `base` clamps every wait to `cap`, which is a degenerate
	/// but harmless way to ask for a fixed interval.
	pub const fn new(base: SignedDuration, cap: SignedDuration) -> Self {
		Backoff { base, cap }
	}

	pub const fn base(self) -> SignedDuration {
		self.base
	}

	pub const fn cap(self) -> SignedDuration {
		self.cap
	}

	/// How long to wait after `attempts` consecutive failures.
	///
	/// A zeroth attempt is treated as the first rather than as no wait, so
	/// callers whose counter is incremented after the fact still get a delay.
	pub fn after(self, attempts: u32) -> SignedDuration {
		let doublings = attempts.saturating_sub(1).min(MAX_DOUBLINGS);
		self.base
			.saturating_mul(2i32.saturating_pow(doublings))
			.min(self.cap)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const SCHEDULE: Backoff =
		Backoff::new(SignedDuration::from_secs(15), SignedDuration::from_mins(15));

	#[test]
	fn doubles_from_the_base() {
		assert_eq!(SCHEDULE.after(1), SignedDuration::from_secs(15));
		assert_eq!(SCHEDULE.after(2), SignedDuration::from_secs(30));
		assert_eq!(SCHEDULE.after(3), SignedDuration::from_secs(60));
		assert_eq!(SCHEDULE.after(4), SignedDuration::from_secs(120));
	}

	#[test]
	fn a_zeroth_attempt_waits_as_the_first_does() {
		assert_eq!(SCHEDULE.after(0), SCHEDULE.after(1));
	}

	#[test]
	fn holds_at_the_cap() {
		// 15s doubles past 15min on the seventh attempt (15s * 2^6 = 16min).
		assert_eq!(SCHEDULE.after(7), SignedDuration::from_mins(15));
		assert_eq!(SCHEDULE.after(50), SignedDuration::from_mins(15));
		// Far past where the multiplier itself would overflow.
		assert_eq!(SCHEDULE.after(u32::MAX), SignedDuration::from_mins(15));
	}

	#[test]
	fn a_long_base_does_not_overflow_into_a_short_wait() {
		let long = Backoff::new(
			SignedDuration::from_hours(1),
			SignedDuration::from_hours(12),
		);
		assert_eq!(long.after(1), SignedDuration::from_hours(1));
		assert_eq!(long.after(2), SignedDuration::from_hours(2));
		assert_eq!(long.after(u32::MAX), SignedDuration::from_hours(12));
	}
}
