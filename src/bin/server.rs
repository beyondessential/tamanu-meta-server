use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
	/// Whether to run all background tasks in-process
	///
	/// In production, you should set this and run the tasks in separate
	/// processes, and scale this service horizontally.
	#[structopt(long, default_value = "true")]
	api_only: bool,
}

#[rocket::main]
async fn main() {
	let args = Args::parse();
	tamanu_meta::server(!args.api_only).await;
}
