//! A tiny in-process fixed-window rate limiter, shared via `AppState`. Used to
//! bound abuse of the unauthenticated enrollment endpoints (per source IP and
//! per target server). It's deliberately simple — a `HashMap` of fixed windows
//! behind a `Mutex`; this is a single-process backstop, not a distributed
//! limiter. Behind multiple replicas each gets its own window, which is fine
//! for the threat (slowing a token-guesser / griefer), not a hard quota.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct Window {
	start: Instant,
	count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct RateLimiter {
	windows: Arc<Mutex<HashMap<String, Window>>>,
}

impl RateLimiter {
	/// Record a hit on `key` and report whether it is still within `limit` for
	/// the current `window`. Returns `true` when allowed, `false` when the
	/// caller has exceeded the limit. Expired windows reset on access; entries
	/// for keys idle longer than `window` are pruned opportunistically to keep
	/// the map bounded.
	/// Whether `key` has already exhausted `limit` within the current
	/// `window`, **without** recording a hit.
	///
	/// [`Self::check`] can only report an exhausted budget by spending one,
	/// which means the caller has to do the work first. This lets a caller
	/// turn a client away before doing the work the budget exists to bound.
	pub fn exceeded(&self, key: &str, limit: u32, window: Duration) -> bool {
		let now = Instant::now();
		let map = self.windows.lock().expect("rate-limiter mutex");
		map.get(key)
			.is_some_and(|w| now.duration_since(w.start) <= window && w.count >= limit)
	}

	pub fn check(&self, key: &str, limit: u32, window: Duration) -> bool {
		let now = Instant::now();
		let mut map = self.windows.lock().expect("rate-limiter mutex");

		map.retain(|_, w| now.duration_since(w.start) <= window);

		let entry = map.entry(key.to_owned()).or_insert(Window {
			start: now,
			count: 0,
		});
		if now.duration_since(entry.start) > window {
			entry.start = now;
			entry.count = 0;
		}
		entry.count += 1;
		entry.count <= limit
	}
}
