//! Canopy monitoring pod: the minute-cadence, DB-only sweeps that detect
//! problems and file/close incidents. One loop, one deployment — every sweep
//! here is a cheap read against the same DB, so there's no reason to stand up
//! a separate pod per check.
//!
//! Sweeps run each tick:
//! - **staleness** — look at every monitored server's report freshness against
//!   its down threshold and file (or close) the `(source=canopy,
//!   ref=reachability)` check when nothing at all is reporting, plus a
//!   `stale/<source>` check per reporting source that has gone quiet.
//!   Independent of pingtask: most servers push their own status, so the sweep
//!   operates on the resulting rows regardless of which path produced them.
//! - **backup staleness + reconcile** — `database::backup::sweep`: stale
//!   reported runs / maintenance, and report-vs-inventory reconciliation.
//! - **tailnet key-expiry** — when the Tailscale directory is configured.
//!
//! Plus a startup **incident reconciliation** pass (see below).

use std::time::Duration;

use clap::Parser;
use commons_servers::{
	tailnet_directory::{TailnetDirectory, TailnetDirectoryConfig},
	tailnet_sweeps,
};
use database::{issues::reconcile_open_incidents, statuses::Status};
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info, warn};

/// Walk every open issue through `re_evaluate_incident_membership` so
/// the recorded incident state matches what the current code says it
/// should be.
///
/// The reachability binary is the natural home for this: it already
/// owns the canopy/reachability sweep and runs in its own pod, so a
/// startup pass doesn't slow API request handling.
///
/// Reasons to do this on every startup rather than reach for a one-off:
///
/// - **Code changes drift the rules.** When the join/leave logic
///   changes (new gates like `is_monitored`, `silenced`, …), existing
///   open incidents that no longer satisfy the rules stay open until
///   the next event arrives — which may be never for the servers in
///   question. PR #170 hit exactly this: the migration's value bump
///   tripped the OLD code's reachability sweep during the deploy
///   window, opening 22 spurious incidents on unmonitored servers
///   that the NEW code's rules would never have opened, and which
///   nothing in the steady-state event flow would reconcile.
/// - **Idempotent and cheap when consistent.** `re_evaluate_incident_membership`
///   short-circuits in the `_ => {}` arm when the issue is already in
///   the right state — so a clean DB only pays the read cost.
async fn reconcile_on_startup(pool: &database::Db) {
	let Ok(mut db) = pool.get().await else {
		warn!("incident reconciliation: failed to get database connection");
		return;
	};
	match reconcile_open_incidents(&mut db).await {
		Ok((0, 0)) => debug!("incident reconciliation: nothing to walk"),
		Ok((servers, issues)) => {
			info!(
				"incident reconciliation: walked {issues} open issue(s) across \
				 {servers} server(s)"
			);
		}
		Err(err) => warn!("incident reconciliation: failed: {err}"),
	}
}

/// One-shot backfill of check-stability records from status history. Runs
/// as its own task so a multi-minute replay never delays the sweeps; the
/// marker table and advisory lock inside make it safe to fire on every
/// startup and from several pods at once. Deliberately not a data
/// migration: a single fleet-wide transaction would hold FK row locks on
/// live issues rows for its whole run, blocking ingestion filings.
///
/// TODO(backfill-removal): transitional; delete this (and its call
/// below) once every deployment has run it — see
/// `database::stability::backfill_from_statuses`.
async fn backfill_stability_on_startup(pool: &database::Db) {
	let Ok(mut db) = pool.get().await else {
		warn!("stability backfill: failed to get database connection");
		return;
	};
	match database::stability::backfill_from_statuses(&mut db).await {
		Ok(None) => debug!("stability backfill: already done (or another pod is on it)"),
		Ok(Some(n)) => info!("stability backfill: replayed history into {n} state record(s)"),
		Err(err) => warn!("stability backfill: failed (will retry next startup): {err}"),
	}
}

/// How often the deferred incident-reeval worker drains its queue. Short so
/// incidents open/close promptly after a status push; work is coalesced per
/// server, so a tight cadence is cheap.
const REEVAL_INTERVAL: Duration = Duration::from_secs(2);

/// Backstop cap on servers drained per reeval tick, so one tick can't hog the
/// pod. The queue coalesces per server, so this is rarely reached.
const REEVAL_BATCH: i64 = 256;

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		reconcile_on_startup(&pool).await;
		let backfill_pool = pool.clone();
		task::spawn(async move { backfill_stability_on_startup(&backfill_pool).await });

		// Deferred incident (re-)evaluation worker. The status-ingest path
		// enqueues servers instead of evaluating incident membership inline
		// (which took the per-group `server_groups` lock and convoyed the
		// fleet under load). This single worker drains the queue on a short
		// cadence, so it — not request traffic — is the only taker of that
		// lock, and incidents still open/close promptly. The main loop below
		// drains too, as a backstop if this task ever dies.
		let reeval_pool = pool.clone();
		task::spawn(async move {
			loop {
				sleep(REEVAL_INTERVAL).await;
				let Ok(mut db) = reeval_pool.get().await else {
					error!("reeval worker: failed to get database connection");
					continue;
				};
				match database::issues::process_incident_reeval_queue(&mut db, REEVAL_BATCH).await {
					Ok(0) => {}
					Ok(n) => debug!("reeval worker: processed {n} queued server(s)"),
					Err(err) => error!("incident reeval worker failed: {err}"),
				}
			}
		});

		// The directory is optional: in dev / single-tenant deploys the
		// TAILSCALE_* env vars aren't set, and the key-expiry sweep just
		// no-ops. Build it once at startup so we don't re-mint OAuth
		// tokens every cycle.
		let directory = match TailnetDirectoryConfig::from_env() {
			Ok(Some(config)) => match TailnetDirectory::new(config).await {
				Ok(d) => {
					info!("tailnet directory loaded; key-expiry sweep enabled");
					Some(d)
				}
				Err(err) => {
					error!("tailnet directory init failed; key-expiry sweep disabled: {err}");
					None
				}
			},
			Ok(None) => {
				info!("tailnet directory not configured; key-expiry sweep disabled");
				None
			}
			Err(err) => {
				error!("tailnet directory config invalid; key-expiry sweep disabled: {err}");
				None
			}
		};

		loop {
			sleep(Duration::from_secs(60)).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};

			match Status::sweep_staleness(&mut db).await {
				Ok(0) => {}
				Ok(n) => debug!("filed {n} staleness events"),
				Err(err) => error!("staleness sweep failed: {err}"),
			}

			// Fleet-wide check liveness: refresh each catalogued check's
			// last_seen and re-animate any decommissioned check that has
			// reported again. Off the hot ingestion path, minute cadence.
			match database::check_policies::CheckPolicy::reconcile_liveness(&mut db).await {
				Ok(0) => {}
				Ok(n) => debug!("re-animated {n} decommissioned check(s)"),
				Err(err) => error!("check liveness reconcile failed: {err}"),
			}

			// Canopy-wide warning for checks gone quiet across the whole
			// fleet. Runs after liveness so it reads fresh last_seen.
			match database::self_alerts::sweep_stale_healthchecks(&mut db).await {
				Ok(_) => {}
				Err(err) => error!("stale-healthcheck self-alert sweep failed: {err}"),
			}

			// Incidents whose linger window has expired: the last effective
			// failure left, nothing came back, close them and ship the
			// pending Slack cancel-or-resolve.
			match database::issues::sweep_lingering_incidents(&mut db).await {
				Ok(0) => {}
				Ok(n) => debug!("closed {n} lingering incident(s)"),
				Err(err) => error!("incident linger sweep failed: {err}"),
			}

			// Backstop drain of the incident-reeval queue in case the dedicated
			// worker task above has died. Safe to run concurrently with it:
			// `process_incident_reeval_queue` claims rows `FOR UPDATE SKIP
			// LOCKED`, so the two never double-process a server.
			match database::issues::process_incident_reeval_queue(&mut db, REEVAL_BATCH).await {
				Ok(0) => {}
				Ok(n) => debug!("reeval backstop: processed {n} queued server(s)"),
				Err(err) => error!("incident reeval backstop failed: {err}"),
			}

			// Backup staleness + report-vs-inventory reconciliation: another
			// minute-cadence DB-only sweep, so it rides this loop rather than a
			// separate pod.
			match database::backup::sweep(&mut db).await {
				Ok(0) => {}
				Ok(n) => debug!("filed {n} backup staleness/reconcile events"),
				Err(err) => error!("backup staleness sweep failed: {err}"),
			}

			if let Some(directory) = &directory {
				match tailnet_sweeps::sweep_key_expiry(&mut db, directory).await {
					Ok(0) => {}
					Ok(n) => debug!("filed {n} tailscale key-expiry events"),
					Err(err) => error!("tailnet key-expiry sweep failed: {err}"),
				}
			}

			// MCP bearer tokens near their fixed one-year expiry: one
			// coalescing issue on the nil/meta server (see `sweep_token_expiry`).
			match database::mcp_tokens::sweep_token_expiry(&mut db).await {
				Ok(0) => {}
				Ok(n) => debug!("filed {n} mcp token-expiry events"),
				Err(err) => error!("mcp token-expiry sweep failed: {err}"),
			}
		}
	})
}

#[derive(Debug, Parser)]
struct Args {
	#[command(flatten)]
	logging: LoggingArgs,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
	let mut _guard = PreArgs::parse().setup()?;
	let args = Args::parse();
	if _guard.is_none() {
		_guard = Some(args.logging.setup(|v| match v {
			0 => "info",
			1 => "debug",
			_ => "trace",
		})?);
	}

	spawn().await.into_diagnostic()?;
	Ok(())
}
