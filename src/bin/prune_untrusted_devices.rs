use clap::Parser;
use diesel_async::SimpleAsyncConnection as _;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;

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

	let db = tamanu_meta::state::AppState::init_db()?;
	let mut conn = db.get().await.into_diagnostic()?;
	conn.batch_execute("SELECT prune_untrusted_devices();")
		.await
		.into_diagnostic()?;
	Ok(())
}
