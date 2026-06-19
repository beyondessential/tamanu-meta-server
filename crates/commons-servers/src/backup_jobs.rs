//! Shared helpers for the backup-credentials scheduler binaries
//! (`backup_preflight` here, plus maintenance/inspection in the sibling jobs
//! component). Kept in `commons-servers` so all three schedulers agree on the
//! jitter scheme.

use std::time::Duration;

use commons_types::Uuid;

/// Stable per-group jitter slot: `hash(group_id) mod window`.
///
/// Spreads per-group work (maintenance, inspection, preflight) evenly across
/// the cadence window so the fleet doesn't stampede at the top of the hour.
/// Derived deterministically from the group UUID's bytes, so it is stable
/// across restarts and identical in every scheduler — a given group always
/// lands in the same slot.
pub fn jitter_slot(group_id: Uuid, window: Duration) -> Duration {
	let window_secs = window.as_secs().max(1);
	let bytes = group_id.as_bytes();
	// Fold both halves so any byte difference changes the slot (UUIDs that
	// differ only in their low bytes must not collide).
	let hi = u64::from_be_bytes(bytes[..8].try_into().expect("uuid is 16 bytes"));
	let lo = u64::from_be_bytes(bytes[8..].try_into().expect("uuid is 16 bytes"));
	Duration::from_secs((hi ^ lo) % window_secs)
}

/// Whether `now` (as a count of seconds into the window) falls in this group's
/// jittered slot for a tick of length `tick`. Used by the minute-cadence
/// preflight loop to fire a group's hourly deep check on the right tick.
pub fn slot_is_due(group_id: Uuid, window: Duration, tick: Duration, secs_into_window: u64) -> bool {
	let slot = jitter_slot(group_id, window).as_secs();
	let tick_secs = tick.as_secs().max(1);
	// True when secs_into_window is within [slot, slot + tick).
	secs_into_window >= slot && secs_into_window < slot + tick_secs
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn jitter_is_stable_and_bounded() {
		let g = Uuid::from_u128(0x1234_5678_9abc_def0_1122_3344_5566_7788);
		let window = Duration::from_secs(3600);
		let a = jitter_slot(g, window);
		let b = jitter_slot(g, window);
		assert_eq!(a, b, "stable per group");
		assert!(a.as_secs() < 3600, "within the window");
	}

	#[test]
	fn different_groups_can_get_different_slots() {
		let window = Duration::from_secs(3600);
		let a = jitter_slot(Uuid::from_u128(1), window);
		let b = jitter_slot(Uuid::from_u128(2), window);
		assert_ne!(a, b);
	}

	#[test]
	fn slot_due_only_in_its_tick() {
		let g = Uuid::from_u128(7);
		let window = Duration::from_secs(3600);
		let tick = Duration::from_secs(60);
		let slot = jitter_slot(g, window).as_secs();
		assert!(slot_is_due(g, window, tick, slot));
		assert!(slot_is_due(g, window, tick, slot + 59));
		assert!(!slot_is_due(g, window, tick, slot + 60));
		if slot >= 1 {
			assert!(!slot_is_due(g, window, tick, slot - 1));
		}
	}
}
