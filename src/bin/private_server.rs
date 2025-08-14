use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
	#[arg(long, default_value = "/$")]
	prefix: String,

	#[arg(long, short, default_value = "8081", env = "PORT")]
	port: u16,

	#[arg(long, env = "BIND_ADDRESS", conflicts_with = "port")]
	bind: Option<SocketAddr>,
}

#[tokio::main]
async fn main() -> tamanu_meta::error::Result<()> {
	let args = Args::parse();
	let addr = args
		.bind
		.unwrap_or_else(|| SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, args.port, 0, 0)));
	tamanu_meta::serve(tamanu_meta::private_routes(args.prefix), addr).await
}
