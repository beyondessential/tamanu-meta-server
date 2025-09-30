use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use axum_client_ip::ClientIpSource;
use clap::Parser;
use commons_servers::{router, serve};
use lloggs::{LoggingArgs, PreArgs};
use private_server::state::AppState;

#[derive(Debug, Parser)]
struct Args {
	#[command(flatten)]
	logging: LoggingArgs,

	#[arg(long, default_value = "/$")]
	prefix: String,

	#[arg(long, short, default_value = "8081", env = "PORT")]
	port: u16,

	#[arg(long, env = "BIND_ADDRESS", conflicts_with = "port")]
	bind: Option<SocketAddr>,

	#[arg(long, env = "CLIENT_IP_SOURCE", default_value = "ConnectInfo")]
	client_ip_source: ClientIpSource,
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

	let addr = args
		.bind
		.unwrap_or_else(|| SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, args.port, 0, 0)));

	serve(
		router(
			private_server::routes(args.prefix).with_state(AppState::init()?),
			args.client_ip_source,
		),
		addr,
	)
	.await?;
	Ok(())
}
