//! Read-through cache mapping a tailnet IP → its node identity, backed
//! by the Tailscale control-plane REST API.
//!
//! The private-server's device-auth extractor uses this to translate an
//! incoming request's `X-Forwarded-For` (set by the Tailscale Operator
//! ingress proxy to the caller's CGNAT v4 or ULA v6 address) into a
//! stable `node_id`, which then keys into the `devices.tailscale_node_id`
//! column.
//!
//! The directory is constructed only on private-server (public-server's
//! `AppState` doesn't carry one), so the tailnet auth path can't fire on
//! the internet edge by type-system construction.

use std::{
	collections::HashMap,
	net::IpAddr,
	sync::Arc,
	time::{Duration, Instant},
};

use commons_errors::{AppError, Result};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;

/// Resolved node identity for a tailnet IP.
#[derive(Clone, Debug)]
pub struct DirectoryEntry {
	pub node_id: String,
	pub node_name: String,
	pub tailnet: String,
	pub tags: Vec<String>,
}

/// Configuration for constructing a [`TailnetDirectory`].
#[derive(Clone, Debug)]
pub struct TailnetDirectoryConfig {
	pub oauth_client_id: String,
	pub oauth_client_secret: String,
	/// Tailnet identifier — typically the email-like ID shown in the
	/// admin console (e.g. `example.com` or `tailbeef-bes.au`).
	pub tailnet: String,
	/// Base URL for the Tailscale REST API. Defaults to
	/// `https://api.tailscale.com`; overridable for testing.
	pub api_base: String,
	pub refresh_period: Duration,
	pub miss_cooldown: Duration,
}

impl TailnetDirectoryConfig {
	/// Read the config from environment variables. Returns `Ok(None)` when
	/// the required vars are unset — that's the "tailnet auth disabled"
	/// path, and callers should treat it as a normal state.
	pub fn from_env() -> Result<Option<Self>> {
		let client_id = std::env::var("TAILSCALE_OAUTH_CLIENT_ID").ok();
		let client_secret = std::env::var("TAILSCALE_OAUTH_CLIENT_SECRET").ok();
		let tailnet = std::env::var("TAILSCALE_TAILNET").ok();

		match (client_id, client_secret, tailnet) {
			(Some(id), Some(secret), Some(net)) => Ok(Some(Self {
				oauth_client_id: id,
				oauth_client_secret: secret,
				tailnet: net,
				api_base: std::env::var("TAILSCALE_API_BASE")
					.unwrap_or_else(|_| "https://api.tailscale.com".into()),
				refresh_period: Duration::from_secs(60),
				miss_cooldown: Duration::from_secs(5),
			})),
			(None, None, None) => Ok(None),
			_ => Err(AppError::custom(
				"TAILSCALE_OAUTH_CLIENT_ID, TAILSCALE_OAUTH_CLIENT_SECRET, and TAILSCALE_TAILNET must all be set together",
			)),
		}
	}
}

#[derive(Clone, Debug)]
pub struct TailnetDirectory {
	inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
	client: Client,
	config: TailnetDirectoryConfig,
	cache: RwLock<Cache>,
	oauth: RwLock<OAuthState>,
}

#[derive(Debug, Default)]
struct Cache {
	by_ip: HashMap<IpAddr, DirectoryEntry>,
	last_refresh: Option<Instant>,
}

#[derive(Debug, Default)]
struct OAuthState {
	token: Option<OAuthToken>,
}

#[derive(Clone, Debug)]
struct OAuthToken {
	access_token: String,
	expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct OAuthResponse {
	access_token: String,
	#[serde(default)]
	expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct DevicesResponse {
	devices: Vec<DeviceRecord>,
}

#[derive(Debug, Deserialize)]
struct DeviceRecord {
	#[serde(rename = "nodeId")]
	node_id: String,
	#[serde(default)]
	name: String,
	#[serde(default)]
	addresses: Vec<String>,
	#[serde(default)]
	tags: Vec<String>,
	#[serde(rename = "tailnetName", default)]
	tailnet_name: String,
}

impl TailnetDirectory {
	/// Build a directory and kick off the background refresh loop. The
	/// first refresh is awaited synchronously so the directory has a
	/// populated cache before the server starts serving — operators who
	/// misconfigure the credentials get a clear startup error rather than
	/// per-request 503s.
	pub async fn new(config: TailnetDirectoryConfig) -> Result<Self> {
		let client = Client::builder()
			.timeout(Duration::from_secs(15))
			.build()
			.map_err(|e| AppError::custom(format!("building reqwest client: {e}")))?;

		let directory = Self {
			inner: Arc::new(Inner {
				client,
				config,
				cache: RwLock::default(),
				oauth: RwLock::default(),
			}),
		};

		directory.refresh().await?;

		let bg = directory.clone();
		tokio::spawn(async move {
			let period = bg.inner.config.refresh_period;
			loop {
				tokio::time::sleep(period).await;
				if let Err(e) = bg.refresh().await {
					tracing::warn!(error = %e, "tailnet directory refresh failed");
				}
			}
		});

		Ok(directory)
	}

	/// Lookup a tailnet IP. Returns `None` for unknown IPs. On a miss the
	/// directory will at most one refresh per `miss_cooldown` to pick up
	/// newly-joined nodes.
	pub async fn lookup(&self, ip: IpAddr) -> Result<Option<DirectoryEntry>> {
		{
			let cache = self.inner.cache.read().await;
			if let Some(entry) = cache.by_ip.get(&ip) {
				return Ok(Some(entry.clone()));
			}
			if let Some(last) = cache.last_refresh
				&& last.elapsed() < self.inner.config.miss_cooldown
			{
				return Ok(None);
			}
		}

		self.refresh().await?;

		let cache = self.inner.cache.read().await;
		Ok(cache.by_ip.get(&ip).cloned())
	}

	/// Force-refresh the cache from the Tailscale control plane.
	pub async fn refresh(&self) -> Result<()> {
		let token = self.access_token().await?;
		let url = format!(
			"{}/api/v2/tailnet/{}/devices",
			self.inner.config.api_base.trim_end_matches('/'),
			self.inner.config.tailnet,
		);
		let resp = self
			.inner
			.client
			.get(&url)
			.bearer_auth(&token)
			.send()
			.await
			.map_err(|e| AppError::custom(format!("tailscale devices request: {e}")))?;

		if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
			// Token expired between the time we minted it and now. Drop and
			// retry exactly once.
			self.inner.oauth.write().await.token = None;
			let token = self.access_token().await?;
			let retry = self
				.inner
				.client
				.get(&url)
				.bearer_auth(&token)
				.send()
				.await
				.map_err(|e| AppError::custom(format!("tailscale devices retry: {e}")))?;
			return self.ingest(retry).await;
		}

		self.ingest(resp).await
	}

	async fn ingest(&self, resp: reqwest::Response) -> Result<()> {
		if !resp.status().is_success() {
			let status = resp.status();
			let body = resp.text().await.unwrap_or_default();
			return Err(AppError::custom(format!(
				"tailscale devices: {status}: {body}"
			)));
		}
		let parsed: DevicesResponse = resp
			.json()
			.await
			.map_err(|e| AppError::custom(format!("decoding devices response: {e}")))?;

		let mut by_ip = HashMap::with_capacity(parsed.devices.len() * 2);
		for d in parsed.devices {
			let entry = DirectoryEntry {
				node_id: d.node_id,
				node_name: d.name,
				tailnet: d.tailnet_name,
				tags: d.tags,
			};
			for addr in d.addresses {
				if let Ok(ip) = addr.parse::<IpAddr>() {
					by_ip.insert(ip, entry.clone());
				}
			}
		}

		let mut cache = self.inner.cache.write().await;
		cache.by_ip = by_ip;
		cache.last_refresh = Some(Instant::now());
		Ok(())
	}

	async fn access_token(&self) -> Result<String> {
		{
			let state = self.inner.oauth.read().await;
			if let Some(tok) = &state.token
				&& tok.expires_at > Instant::now() + Duration::from_secs(30)
			{
				return Ok(tok.access_token.clone());
			}
		}

		let url = format!(
			"{}/api/v2/oauth/token",
			self.inner.config.api_base.trim_end_matches('/'),
		);
		let body = serde_urlencoded::to_string([
			("grant_type", "client_credentials"),
			("client_id", &self.inner.config.oauth_client_id),
			("client_secret", &self.inner.config.oauth_client_secret),
		])
		.map_err(|e| AppError::custom(format!("encoding oauth body: {e}")))?;

		let resp = self
			.inner
			.client
			.post(&url)
			.header("Content-Type", "application/x-www-form-urlencoded")
			.body(body)
			.send()
			.await
			.map_err(|e| AppError::custom(format!("tailscale oauth request: {e}")))?;

		if !resp.status().is_success() {
			let status = resp.status();
			let body = resp.text().await.unwrap_or_default();
			return Err(AppError::custom(format!(
				"tailscale oauth: {status}: {body}"
			)));
		}

		let parsed: OAuthResponse = resp
			.json()
			.await
			.map_err(|e| AppError::custom(format!("decoding oauth response: {e}")))?;

		let expires_at = Instant::now() + Duration::from_secs(parsed.expires_in.max(60));
		let token = OAuthToken {
			access_token: parsed.access_token.clone(),
			expires_at,
		};

		let mut state = self.inner.oauth.write().await;
		state.token = Some(token);
		Ok(parsed.access_token)
	}

	/// Construct a directory pre-populated with fixed entries, for tests.
	/// No background refresh, no API calls. The entries are indexed by
	/// each address they list.
	pub fn for_test(entries: impl IntoIterator<Item = (IpAddr, DirectoryEntry)>) -> Self {
		let mut by_ip = HashMap::new();
		for (ip, entry) in entries {
			by_ip.insert(ip, entry);
		}
		let cache = Cache {
			by_ip,
			last_refresh: Some(Instant::now()),
		};
		Self {
			inner: Arc::new(Inner {
				client: Client::new(),
				config: TailnetDirectoryConfig {
					oauth_client_id: String::new(),
					oauth_client_secret: String::new(),
					tailnet: String::new(),
					api_base: String::new(),
					refresh_period: Duration::from_secs(3600),
					miss_cooldown: Duration::from_secs(3600),
				},
				cache: RwLock::new(cache),
				oauth: RwLock::default(),
			}),
		}
	}
}

/// True for any IP that's in Tailscale's CGNAT v4 range (100.64.0.0/10) or
/// its ULA v6 prefix (fd7a:115c:a1e0::/48). Used as the spoof-guard
/// for the dual-auth tailnet path: only consider X-Forwarded-For values
/// in these ranges as possibly identifying a tailnet caller.
pub fn is_tailnet_ip(ip: IpAddr) -> bool {
	match ip {
		IpAddr::V4(v4) => {
			let oct = v4.octets();
			oct[0] == 100 && (oct[1] & 0b1100_0000) == 0b0100_0000
		}
		IpAddr::V6(v6) => {
			let seg = v6.segments();
			seg[0] == 0xfd7a && seg[1] == 0x115c && seg[2] == 0xa1e0
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::net::{Ipv4Addr, Ipv6Addr};

	#[test]
	fn cgnat_v4_is_tailnet() {
		assert!(is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
		assert!(is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(100, 127, 255, 254))));
		assert!(is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(100, 100, 50, 50))));
	}

	#[test]
	fn non_cgnat_v4_is_not_tailnet() {
		assert!(!is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
		assert!(!is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
		assert!(!is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(100, 63, 255, 255))));
		assert!(!is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(100, 128, 0, 0))));
		assert!(!is_tailnet_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
	}

	#[test]
	fn tailscale_ula_is_tailnet() {
		assert!(is_tailnet_ip(IpAddr::V6(
			"fd7a:115c:a1e0::3701:2c8a".parse::<Ipv6Addr>().unwrap()
		)));
		assert!(is_tailnet_ip(IpAddr::V6(
			"fd7a:115c:a1e0:1:2:3:4:5".parse::<Ipv6Addr>().unwrap()
		)));
	}

	#[test]
	fn other_v6_is_not_tailnet() {
		assert!(!is_tailnet_ip(IpAddr::V6(
			"fd00:1234::1".parse::<Ipv6Addr>().unwrap()
		)));
		assert!(!is_tailnet_ip(IpAddr::V6(
			"::1".parse::<Ipv6Addr>().unwrap()
		)));
		assert!(!is_tailnet_ip(IpAddr::V6(
			"2001:db8::1".parse::<Ipv6Addr>().unwrap()
		)));
	}

	#[test]
	fn for_test_lookup() {
		let entry = DirectoryEntry {
			node_id: "n1".into(),
			node_name: "alpha".into(),
			tailnet: "test".into(),
			tags: vec!["tag:server".into()],
		};
		let ip: IpAddr = "100.64.0.42".parse().unwrap();
		let dir = TailnetDirectory::for_test([(ip, entry.clone())]);
		let runtime = tokio::runtime::Runtime::new().unwrap();
		let resolved = runtime.block_on(dir.lookup(ip)).unwrap().unwrap();
		assert_eq!(resolved.node_id, "n1");
		assert_eq!(resolved.tags, vec!["tag:server".to_string()]);
		let missing: IpAddr = "100.64.0.99".parse().unwrap();
		assert!(runtime.block_on(dir.lookup(missing)).unwrap().is_none());
	}
}
