#[cfg(feature = "ssr")]
use chrono::{DateTime, Utc};
#[cfg(feature = "ssr")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "ssr")]
use std::sync::{Arc, RwLock};

#[cfg(feature = "ssr")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromeRelease {
	pub name: String,
	#[serde(rename = "releaseDate")]
	pub release_date: String,
	#[serde(rename = "isEol")]
	pub is_eol: bool,
	#[serde(rename = "eolFrom")]
	pub eol_from: Option<String>,
}

#[cfg(feature = "ssr")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChromeApiResponse {
	result: ChromeResult,
}

#[cfg(feature = "ssr")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChromeResult {
	releases: Vec<ChromeRelease>,
}

#[cfg(feature = "ssr")]
#[derive(Debug, Default, Clone)]
pub struct ChromeVersionCache {
	cache: Arc<RwLock<Option<Arc<Vec<ChromeRelease>>>>>,
}

#[cfg(feature = "ssr")]
impl ChromeVersionCache {
	pub fn new() -> Self {
		Self {
			cache: Arc::new(RwLock::new(None)),
		}
	}

	pub async fn get_supported_versions_at_date(
		&self,
		date: DateTime<Utc>,
	) -> Result<Vec<u32>, Box<dyn std::error::Error + Send + Sync>> {
		// Clone Arc with minimal read lock duration
		let releases = {
			let cache = self.cache.read().unwrap();
			cache.as_ref().ok_or("Cache not initialized")?.clone()
		};

		let date_str = date.format("%Y-%m-%d").to_string();

		let supported: Vec<u32> = releases
			.iter()
			.filter_map(|release| {
				let release_date = &release.release_date;

				if release_date <= &date_str {
					let eol_date = release.eol_from.as_ref();
					if eol_date.is_none() || eol_date.unwrap() > &date_str {
						release.name.parse::<u32>().ok()
					} else {
						None
					}
				} else {
					None
				}
			})
			.collect();

		Ok(supported)
	}

	pub async fn fetch(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.fetch_and_update().await
	}

	async fn fetch_and_update(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let url = "https://endoflife.date/api/v1/products/chrome/";

		// Fetch outside the write lock
		let response = reqwest::get(url).await?;
		let data: ChromeApiResponse = response.json().await?;

		// Only hold write lock for the minimal time needed to update
		let mut cache = self.cache.write().unwrap();
		*cache = Some(Arc::new(data.result.releases));

		Ok(())
	}
}
