use axum::extract::FromRef;
use commons_errors::Result;
use database::Db;
use leptos::config::{LeptosOptions, get_configuration};

use crate::chrome_cache::ChromeVersionCache;

#[derive(Clone, Debug, FromRef)]
pub struct AppState {
	pub db: Db,
	pub leptos_options: LeptosOptions,
	pub chrome_cache: ChromeVersionCache,
}

impl AppState {
	pub fn init() -> Result<Self> {
		let conf = get_configuration(None).unwrap();

		let chrome_cache = ChromeVersionCache::new();

		// Spawn background task to refresh Chrome version cache daily
		{
			let cache = chrome_cache.clone();
			tokio::spawn(async move {
				// Initial fetch
				if let Err(e) = cache.fetch().await {
					tracing::error!("Failed to fetch Chrome versions initially: {}", e);
				}

				// Refresh every 24 hours
				let mut interval =
					tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
				interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

				loop {
					interval.tick().await;
					if let Err(e) = cache.fetch().await {
						tracing::error!("Failed to refresh Chrome versions: {}", e);
					}
				}
			});
		}

		Ok(Self {
			db: database::init(),
			leptos_options: conf.leptos_options,
			chrome_cache,
		})
	}

	pub fn from_db_url(url: &str) -> Result<Self> {
		let conf = get_configuration(None).unwrap();

		let chrome_cache = ChromeVersionCache::new();

		// Spawn background task to refresh Chrome version cache daily
		{
			let cache = chrome_cache.clone();
			tokio::spawn(async move {
				// Initial fetch
				if let Err(e) = cache.fetch().await {
					tracing::error!("Failed to fetch Chrome versions initially: {}", e);
				}

				// Refresh every 24 hours
				let mut interval =
					tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
				interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

				loop {
					interval.tick().await;
					if let Err(e) = cache.fetch().await {
						tracing::error!("Failed to refresh Chrome versions: {}", e);
					}
				}
			});
		}

		Ok(Self {
			db: database::init_to(url),
			leptos_options: conf.leptos_options,
			chrome_cache,
		})
	}
}
