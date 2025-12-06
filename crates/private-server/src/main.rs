#[cfg(feature = "ssr")]
#[derive(Debug, clap::Parser)]
struct Args {
	#[command(flatten)]
	logging: lloggs::LoggingArgs,

	#[arg(long, short, default_value = "8081", env = "PORT")]
	port: u16,

	#[arg(long, env = "BIND_ADDRESS", conflicts_with = "port")]
	bind: Option<std::net::SocketAddr>,

	#[arg(long, env = "CLIENT_IP_SOURCE", default_value = "ConnectInfo")]
	client_ip_source: axum_client_ip::ClientIpSource,
}

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> miette::Result<()> {
	use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

	use clap::Parser;
	use commons_servers::{router, serve};
	use lloggs::PreArgs;
	use private_server::state::AppState;

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

	tokio::select! {
		_ = tokio::signal::ctrl_c() => {
			println!(); // when Ctrl+C is pressed, often the terminal will print ^C, and the log line will be indented
			tracing::info!("Received Ctrl+C signal, exiting");
		}
		res = serve(
			router(
				private_server::routes(AppState::init().await?)?,
				args.client_ip_source,
			),
			addr,
		) => {
			tracing::info!("Server exited");
			res?;
		}
	}
	Ok(())
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
	// no client-side main function
}
