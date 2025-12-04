use clap::Parser;
use commons_errors::{AppError, Result};
use database::{
	Db,
	chrome_releases::{ChromeRelease, NewChromeRelease},
};
use lloggs::{LoggingArgs, PreArgs};
use serde::{Deserialize, Serialize};
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

async fn fetch_chrome_versions() -> Result<Vec<ApiChromeRelease>> {
	let url = "https://endoflife.date/api/v1/products/chrome/";
	let response = reqwest::get(url)
		.await
		.map_err(|e| AppError::Custom(e.to_string()))?;
	let data: ChromeApiResponse = response
		.json()
		.await
		.map_err(|e| AppError::Custom(e.to_string()))?;
	Ok(data.result.releases)
}

async fn update_chrome_versions(pool: Db) -> Result<()> {
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
			release_date: release.release_date.clone(),
			is_eol: release.is_eol,
			eol_from: release.eol_from.clone(),
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

	update_chrome_versions(pool).await.map_err(|err| {
		error!("Failed to update Chrome versions: {}", err);
		miette::miette!("{}", err)
	})?;

	Ok(())
}
