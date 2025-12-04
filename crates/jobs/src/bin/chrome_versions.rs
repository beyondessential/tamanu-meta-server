use clap::Parser;
use commons_errors::Result;
use database::{
	Db,
	chrome_releases::{ChromeRelease, NewChromeRelease},
};
use jiff::civil::Date;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing::{debug, error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChromeApiResponse {
	result: ChromeResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChromeResult {
	releases: Vec<ApiChromeRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiChromeRelease {
	name: String,
	#[serde(rename = "releaseDate")]
	release_date: String,
	#[serde(rename = "isEol")]
	is_eol: bool,
	#[serde(rename = "eolFrom")]
	eol_from: Option<String>,
}

async fn fetch_chrome_versions()
-> Result<Vec<ApiChromeRelease>, Box<dyn std::error::Error + Send + Sync>> {
	let url = "https://endoflife.date/api/v1/products/chrome/";
	let response = reqwest::get(url).await?;
	let data: ChromeApiResponse = response.json().await?;
	Ok(data.result.releases)
}

fn parse_date(date_str: &str) -> Result<Date, Box<dyn std::error::Error + Send + Sync>> {
	Date::parse(date_str, "%Y-%m-%d")
		.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
}

async fn update_chrome_versions(pool: Db) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let mut db = pool.get().await?;

	// Fetch from API
	let releases = fetch_chrome_versions().await?;
	info!("Fetched {} Chrome versions from API", releases.len());

	// Delete all existing records
	ChromeRelease::delete_all(&mut db).await?;
	debug!("Deleted all existing Chrome release records");

	// Insert new records
	let mut inserted = 0;
	for release in releases {
		let new_release = NewChromeRelease {
			version: release.name.clone(),
			release_date: parse_date(&release.release_date)?,
			is_eol: release.is_eol,
			eol_from: release
				.eol_from
				.as_ref()
				.map(|d| parse_date(d))
				.transpose()?,
		};

		new_release.save(&mut db).await?;
		inserted += 1;
	}

	info!("Inserted {} Chrome release records into database", inserted);
	Ok(())
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

	let pool = database::init();

	if let Err(err) = update_chrome_versions(pool).await {
		error!("Failed to update Chrome versions: {}", err);
		return Err(err).into_diagnostic();
	}

	Ok(())
}
