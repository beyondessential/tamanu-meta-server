//! The domains pod: wiring and the loops. The sweeps themselves are
//! [`jobs::domains`], so they can be exercised against a fake zone and a fake
//! authority without a pod.
// spec: CRT

use clap::Parser;
use commons_servers::acme::Acme;
use commons_servers::dns_provider::DnsProvider;
use commons_types::dns::ManagedZone;
use jobs::domains::{
	RENEWAL_INTERVAL, WORK_INTERVAL, reconcile_addresses, start_renewals, work_orders,
};
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info};

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	task::spawn(async move {
		// Read once at startup: changing what Canopy manages is a deployment
		// change, and re-reading per tick would let a half-applied configuration
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
					Ok(0) => {}
					Ok(n) => debug!("obtained {n} certificate(s)"),
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
