//! Progress-series pruning. Deletes in-flight backup progress samples
//! ([`database::BackupRunProgress`]) once they age past [`RETENTION`].
//!
//! The series is working data — enough to watch a run as it goes and to review
//! how a past one behaved — not part of a run's permanent record. A run's own
//! outcome, sizes, and traffic totals live on `backup_runs` and are untouched by
//! this, so pruning never loses anything a run reported.
//!
//! Unlike the maintenance and inspection loops, this is fleet-wide rather than
//! per-group: it takes no repository lock, holds no kopia subprocess, and sits
//! outside the one-operation-per-group interlock, so it can never delay a
//! group's real backup work or be delayed by it.

use std::time::Duration;

use database::BackupRunProgress;
use jiff::Timestamp;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, warn};

/// How long a run's progress series is kept. Long enough that a run which
/// misbehaved last week can still be looked at, short enough that the table
/// stays small: a multi-hour run at a per-minute cadence is on the order of a
/// thousand rows, so a fleet's worth over this window is a few million.
pub const RETENTION: Duration = Duration::from_secs(14 * 24 * 3600);

/// How often to sweep. Deliberately slow — the retention boundary is measured in
/// days, so nothing is gained by checking often, and a wide index-ordered delete
/// is better run rarely.
const TICK: Duration = Duration::from_secs(3600);

async fn tick(db: &mut database::diesel_async::AsyncPgConnection, now: Timestamp) {
	let cutoff = now - RETENTION;
	match BackupRunProgress::prune_before(db, cutoff).await {
		Ok(0) => debug!(%cutoff, "progress-prune: nothing to prune"),
		Ok(n) => debug!(%cutoff, "progress-prune: deleted {n} samples"),
		// Best-effort housekeeping: an unbounded table is a slow problem, a
		// failing loop is not an incident. Log and try again next tick.
		Err(e) => warn!(%cutoff, "progress-prune: delete failed: {e}"),
	}
}

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		loop {
			sleep(TICK).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};
			tick(&mut db, Timestamp::now()).await;
		}
	})
}
