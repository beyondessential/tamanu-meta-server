use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
	#[arg(long, default_value = "/$")]
	prefix: String,
}

#[rocket::main]
async fn main() {
	let args = Args::parse();
	tamanu_meta::private_server(args.prefix).await;
}
