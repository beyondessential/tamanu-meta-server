//! The domains pod: wiring and the loops. The sweeps themselves are
//! [`jobs::domains`], so they can be exercised against a fake zone and a fake
//! authority without a pod.
// spec: CRT

use std::time::Duration;

use clap::Parser;
use commons_servers::acme::{Acme, Fault};
use commons_servers::dns_provider::DnsProvider;
use commons_types::dns::ManagedZone;
use jobs::domains::{
	RENEWAL_INTERVAL, Round, WORK_INTERVAL, reconcile_addresses, start_renewals, work_orders,
};
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info, warn};

/// How long to leave the authority alone after it says Canopy is asking too
/// fast. Long enough for a rolling window to move on, short enough that a
/// renewal due today still gets several attempts.
// spec: CRT#when-issuance-fails
const THROTTLED_COOLDOWN: Duration = Duration::from_secs(600);

/// Report whether Canopy can issue at all, from what the round saw.
async fn report_authority_health(pool: &database::Db, round: Round) {
	let Ok(mut db) = pool.get().await else {
		error!("no database connection to report authority health");
		return;
	};
	// The order's own recorded error is the detail; this alert only needs to say
	// which condition it is, since the fix is per-condition rather than per-name.
	if let Err(err) =
		database::self_alerts::sweep_certificate_authority(&mut db, round.fault, None).await
	{
		error!("certificate authority self-alert sweep failed: {err}");
	}
}

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		// Read once at startup: changing what Canopy manages is an instance
		// configuration change, and re-reading per tick would let a half-applied configuration
		// take live records down.
		let zones = match ManagedZone::list_from_env() {
			Ok(zones) if zones.is_empty() => {
				info!(
					"no DNS zones configured (CANOPY_DNS_ZONES); the domains pod will idle. \
					 Groups depending on a zone are reported by the monitor pod."
				);
				Vec::new()
			}
			Ok(zones) => {
				info!(
					zones = ?zones.iter().map(|z| z.apex.as_str()).collect::<Vec<_>>(),
					"managing DNS zones"
				);
				zones
			}
			Err(err) => {
				// Deliberately not fatal: the monitor pod reports the unreadable
				// configuration, and exiting here would strand every registration
				// that is already published.
				error!("DNS zone configuration unreadable; the domains pod will idle: {err}");
				Vec::new()
			}
		};

		// Nothing configured to write to means nothing to build: the AWS client
		// would only go looking for credentials it has no use for.
		let dns = if zones.is_empty() {
			None
		} else {
			Some(DnsProvider::aws().await)
		};

		let acme = match Acme::from_env().await {
			Ok(Some(acme)) => {
				info!(profiles = ?acme.profiles(), "certificate authority ready");
				Some(acme)
			}
			Ok(None) => {
				info!(
					"no ACME account key configured (CANOPY_ACME_ACCOUNT_KEY); certificates will \
					 not be issued. Address records are unaffected."
				);
				None
			}
			Err(err) => {
				// Not fatal either: the address sweeps are useful without it, and
				// crash-looping the pod would stop those too.
				error!(
					"could not reach the certificate authority; certificates will not be issued \
					 until this pod restarts: {err}"
				);
				None
			}
		};

		// Renewals on their own timer, so a fleet-wide sweep does not run at the
		// short cadence the per-registration work wants.
		if acme.is_some() {
			let renewal_pool = pool.clone();
			task::spawn(async move {
				loop {
					sleep(RENEWAL_INTERVAL).await;
					match start_renewals(&renewal_pool).await {
						Ok(0) => {}
						Ok(n) => info!("{n} certificate(s) due to renew"),
						Err(err) => error!("renewal sweep failed: {err}"),
					}
				}
			});
		}

		loop {
			sleep(WORK_INTERVAL).await;

			let Some(dns) = &dns else { continue };

			match reconcile_addresses(&pool, dns, &zones).await {
				Ok(0) => {}
				Ok(n) => debug!("reconciled {n} name(s)"),
				Err(err) => error!("address reconcile failed: {err}"),
			}

			if let Some(acme) = &acme {
				match work_orders(&pool, dns, acme, &zones).await {
					Ok(round) => {
						if round.issued > 0 {
							debug!("obtained {} certificate(s)", round.issued);
						}
						// Whether Canopy can issue at all is Canopy's to report, and
						// separately from any one server's certificate. A round that
						// hit nothing recovers all three conditions.
						// spec: CRT#when-issuance-fails
						report_authority_health(&pool, round).await;

						// A throttled round is followed by a pause, not by the next
						// tick: the limits are shared across every group in the zone,
						// so continuing to ask is how the rest of the allowance goes.
						if round.fault == Some(Fault::Throttled) {
							warn!(
								"the authority is throttling us; holding off orders for {}s",
								THROTTLED_COOLDOWN.as_secs()
							);
							sleep(THROTTLED_COOLDOWN).await;
						}
					}
					Err(err) => error!("certificate sweep failed: {err}"),
				}
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
