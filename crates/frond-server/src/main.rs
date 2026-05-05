use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use clap::Parser;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;

#[derive(Debug, Parser)]
struct Args {
	#[command(flatten)]
	logging: LoggingArgs,

	#[arg(long, short, default_value = "7899", env = "PORT")]
	port: u16,

	#[arg(long, env = "BIND_ADDRESS", conflicts_with = "port")]
	bind: Option<SocketAddr>,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
	let mut _guard = PreArgs::parse_with_env("CANOPY_LOG").setup()?;
	let args = Args::parse();
	if _guard.is_none() {
		_guard = Some(args.logging.setup(|v| match v {
			0 => "info",
			1 => "debug",
			_ => "trace",
		})?);
	}

	rustls::crypto::ring::default_provider()
		.install_default()
		.expect("ring crypto provider already installed");

	let addr = args
		.bind
		.unwrap_or_else(|| SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, args.port, 0, 0)));

	let endpoint = frond_server::bind(addr)?;
	let local = endpoint.local_addr().into_diagnostic()?;
	tracing::info!(%local, "frond-server listening");

	frond_server::accept_loop(endpoint).await;
	Ok(())
}
