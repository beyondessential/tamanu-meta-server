use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use clap::Parser;
use lloggs::{LoggingArgs, PreArgs};

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

	let addr = args
		.bind
		.unwrap_or_else(|| SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, args.port, 0, 0)));

	tracing::info!(%addr, "frond-server scaffold; QUIC listener wiring lands in Phase 2");
	Ok(())
}
