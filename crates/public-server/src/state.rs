#[cfg(feature = "ui")]
use std::sync::Arc;

use axum::extract::FromRef;
use commons_errors::{AppError, Result};
use commons_servers::tailnet_directory::TailnetDirectory;
use database::Db;
#[cfg(feature = "ui")]
use tera::Tera;

/// Narrow wrapper over a [`kube::Client`] + the namespace to read from. The
/// only operation it exposes is `get` on a single named Secret, pulling one
/// key out — the minimal surface `GET /backup-target` needs. It never lists or
/// mutates Secrets.
#[derive(Clone)]
pub struct BackupSecrets {
	client: kube::Client,
	namespace: String,
}

impl std::fmt::Debug for BackupSecrets {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BackupSecrets")
			.field("namespace", &self.namespace)
			.finish_non_exhaustive()
	}
}

impl BackupSecrets {
	pub fn new(client: kube::Client, namespace: String) -> Self {
		Self { client, namespace }
	}

	/// Read one key out of the named Secret in the configured namespace. Maps
	/// every failure (missing Secret, missing key, non-utf8, API error) to
	/// [`AppError::Upstream`] so the handler returns 502 with a generic body.
	pub async fn read_password(&self, secret_name: &str, key: &str) -> Result<String> {
		use k8s_openapi::api::core::v1::Secret;
		use kube::Api;

		let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.namespace);
		let secret = api
			.get(secret_name)
			.await
			.map_err(|e| AppError::Upstream(format!("secret get failed: {e}")))?;

		let data = secret
			.data
			.ok_or_else(|| AppError::Upstream("secret has no data".into()))?;
		let bytes = data
			.get(key)
			.ok_or_else(|| AppError::Upstream(format!("secret has no key {key}")))?;
		String::from_utf8(bytes.0.clone())
			.map_err(|_| AppError::Upstream("secret value is not valid utf-8".into()))
	}
}

#[derive(Clone, Debug)]
pub struct AppState {
	pub db: Db,
	#[cfg(feature = "ui")]
	pub tera: Arc<Tera>,
	#[cfg(feature = "ui")]
	pub server_versions_secret: Option<String>,
	/// Populated only when the public-server's router is nested into
	/// the private-server's `/public/...` mount and the private-server
	/// has a directory configured. The internet-facing public-server
	/// binary's `init()` leaves this `None`, so the tailnet path of
	/// the device-auth extractor can never fire on the open internet.
	pub tailnet_directory: Option<TailnetDirectory>,
	/// In-process rate limiter backing the unauthenticated enrollment
	/// endpoints (per source IP and per target server).
	pub rate_limiter: crate::ratelimit::RateLimiter,
	/// STS client built from the pod's IRSA web-identity. `None` when no AWS
	/// environment is configured (tests, the nested private mount); absent ⇒
	/// `POST /backup-credentials` returns 502 ("issuer not configured").
	pub sts: Option<aws_sdk_sts::Client>,
	/// Kube client for reading repo-password Secrets in canopy's namespace.
	/// `None` in tests / non-cluster runs ⇒ `GET /backup-target` returns 502.
	pub kube: Option<BackupSecrets>,
}

impl AppState {
	#[cfg(feature = "ui")]
	pub fn init_tera() -> Result<Arc<Tera>> {
		let mut tera = Tera::default();

		macro_rules! embed_template {
			($name:expr) => {
				tera.add_raw_template(
					$name,
					include_str!(concat!("../templates/", $name, ".html.tera")),
				)
				.unwrap();
			};
		}

		embed_template!("artifacts");
		embed_template!("mobile");
		embed_template!("password");
		embed_template!("server_versions");
		embed_template!("versions");

		Ok(Arc::new(tera))
	}

	/// Binary entry point. Async because the AWS/kube clients are built from
	/// async provider/cluster discovery. Builds the STS + kube clients from the
	/// pod's IRSA / in-cluster environment; a missing or broken AWS/kube
	/// environment degrades to `None` (the backup endpoints then 502) rather
	/// than failing startup.
	pub async fn init() -> Result<Self> {
		let mut state = Self::from_db(database::init())?;
		state.sts = Some(Self::init_sts().await);
		state.kube = Self::init_kube().await;
		Ok(state)
	}

	/// Build the STS client from the default credential/region provider chain
	/// (in-cluster: the pod's IRSA web-identity).
	async fn init_sts() -> aws_sdk_sts::Client {
		let aws = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
		aws_sdk_sts::Client::new(&aws)
	}

	/// Build the kube-backed secret reader. Returns `None` (logged) when no
	/// cluster config is available, so `/backup-target` 502s until fixed
	/// rather than the binary failing to start.
	async fn init_kube() -> Option<BackupSecrets> {
		match kube::Client::try_default().await {
			Ok(client) => Some(BackupSecrets::new(client, namespace_from_env())),
			Err(err) => {
				tracing::warn!(error = ?err, "kube client unavailable; /backup-target will 502");
				None
			}
		}
	}

	/// Sync constructor with `None` AWS/kube clients — used by the private
	/// server's nested `/public/...` mount, the test harness, and any
	/// non-AWS deployment.
	pub fn from_db(db: Db) -> Result<Self> {
		Self::from_db_with_directory(db, None)
	}

	pub fn from_db_with_directory(
		db: Db,
		tailnet_directory: Option<TailnetDirectory>,
	) -> Result<Self> {
		Ok(Self {
			db,
			#[cfg(feature = "ui")]
			tera: Self::init_tera()?,
			#[cfg(feature = "ui")]
			server_versions_secret: std::env::var("SERVER_VERSIONS_SECRET").ok(),
			tailnet_directory,
			rate_limiter: crate::ratelimit::RateLimiter::default(),
			sts: None,
			kube: None,
		})
	}
}

/// The k8s namespace whose repo-password Secrets `/backup-target` reads, from
/// `POD_NAMESPACE` (inject via the downward API), defaulting to `canopy`.
fn namespace_from_env() -> String {
	std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "canopy".to_string())
}

impl FromRef<AppState> for crate::ratelimit::RateLimiter {
	fn from_ref(state: &AppState) -> Self {
		state.rate_limiter.clone()
	}
}

impl FromRef<AppState> for Db {
	fn from_ref(state: &AppState) -> Self {
		state.db.clone()
	}
}

impl FromRef<AppState> for Option<TailnetDirectory> {
	fn from_ref(state: &AppState) -> Self {
		state.tailnet_directory.clone()
	}
}

impl FromRef<AppState> for Option<aws_sdk_sts::Client> {
	fn from_ref(state: &AppState) -> Self {
		state.sts.clone()
	}
}

impl FromRef<AppState> for Option<BackupSecrets> {
	fn from_ref(state: &AppState) -> Self {
		state.kube.clone()
	}
}

#[cfg(feature = "ui")]
impl FromRef<AppState> for Arc<Tera> {
	fn from_ref(state: &AppState) -> Self {
		state.tera.clone()
	}
}
