//! Canopy monitoring pod: the minute-cadence, DB-only sweeps that detect
//! problems and file/close incidents. One loop, one deployment — every sweep
//! here is a cheap read against the same DB, so there's no reason to stand up
//! a separate pod per check.
//!
//! Sweeps run each tick:
//! - **reachability** — look at every monitored server's latest status row and
//!   file (or close) the `(source=canopy, ref=reachability)` issue. Severity
//!   escalates as the server stays unreported — Notice → Warning at the
//!   sub-incident tiers, then Error (opens an incident) at "Down", then
//!   Critical at "Gone". Independent of pingtask: most servers push their own
//!   status, so the sweep operates on the resulting `statuses` rows regardless
//!   of which path produced them.
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

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		reconcile_on_startup(&pool).await;

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

			match Status::sweep_reachability(&mut db).await {
				Ok(0) => {}
				Ok(n) => debug!("filed {n} reachability events"),
				Err(err) => error!("reachability sweep failed: {err}"),
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

			// MCP bearer tokens near their fixed one-year expiry: fleet-wide,
			// fanned out per group (see `sweep_token_expiry` for why).
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
