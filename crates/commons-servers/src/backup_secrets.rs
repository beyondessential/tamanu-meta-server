//! Shared reader/writer for the per-group repo-password k8s Secrets.
//!
//! Canopy owns every repo passphrase Secret. This narrow store exposes exactly
//! what the components need: read one key (public-server `/backup-target`,
//! jobs maintenance), create one Secret (private-server onboarding), and
//! create-or-replace one (the jobs rotation loop). Real deployments use the
//! `Kube` variant; tests and the e2e binary use the in-memory `Memory` store
//! (gated by `CANOPY_BACKUP_SECRETS_MEMORY`) so these paths are exercised
//! without a cluster.

use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};

use commons_errors::{AppError, Result};

/// Env var that forces the in-memory secret store (no cluster needed). Set by the
/// e2e fixture so onboarding works against the real binary; tests use
/// [`BackupSecrets::memory`] directly.
const MEMORY_ENV: &str = "CANOPY_BACKUP_SECRETS_MEMORY";

type MemoryStore = Arc<Mutex<BTreeMap<String, BTreeMap<String, String>>>>;

/// Reader/writer for the per-group repo-password Secrets. `Kube` is a namespaced
/// [`kube::Client`]; `Memory` is an in-process map for tests + the e2e binary.
#[derive(Clone)]
pub enum BackupSecrets {
	Kube {
		client: kube::Client,
		namespace: String,
	},
	Memory(MemoryStore),
}

impl std::fmt::Debug for BackupSecrets {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Kube { namespace, .. } => f
				.debug_struct("BackupSecrets::Kube")
				.field("namespace", namespace)
				.finish_non_exhaustive(),
			Self::Memory(_) => f.write_str("BackupSecrets::Memory"),
		}
	}
}

impl BackupSecrets {
	pub fn new(client: kube::Client, namespace: String) -> Self {
		Self::Kube { client, namespace }
	}

	/// An in-memory store for tests / the e2e binary. **Debug-only**: the
	/// constructor — and therefore any way to reach the `Memory` variant — does
	/// not exist in release builds, so production secrets can never be backed by a
	/// non-persistent, process-local map, no matter the environment.
	#[cfg(debug_assertions)]
	pub fn memory() -> Self {
		Self::Memory(Arc::new(Mutex::new(BTreeMap::new())))
	}

	/// Build a secret store from the ambient cluster config (in-cluster the pod's
	/// service account; locally `~/.kube/config`), reading from the
	/// `POD_NAMESPACE` namespace (default `canopy`). If `MEMORY_ENV` is set,
	/// returns the in-memory store instead. Returns `None` (logged) when no
	/// cluster config is available, so callers degrade to 502 rather than failing
	/// startup.
	pub async fn try_default() -> Option<Self> {
		if std::env::var_os(MEMORY_ENV).is_some() {
			// The in-memory store is debug-only (tests, e2e, local dev). `memory()`
			// doesn't exist in release builds (attribute-`cfg`, not a runtime check),
			// so it can't be swapped in by accident in production — where backing
			// repo passphrases with a non-persistent, process-local map would be
			// catastrophic.
			#[cfg(debug_assertions)]
			{
				tracing::warn!("{MEMORY_ENV} set; using in-memory backup Secret store");
				return Some(Self::memory());
			}
			#[cfg(not(debug_assertions))]
			tracing::error!(
				"{MEMORY_ENV} is set but IGNORED: the in-memory Secret store is debug-only and is never used in release builds"
			);
		}
		match kube::Client::try_default().await {
			Ok(client) => Some(Self::Kube {
				client,
				namespace: namespace_from_env(),
			}),
			Err(err) => {
				tracing::warn!(error = ?err, "kube client unavailable; backup Secret ops will 502");
				None
			}
		}
	}

	/// Read one key out of the named Secret. Maps every failure (missing Secret,
	/// missing key, non-utf8, API error) to [`AppError::Upstream`] so handlers
	/// return 502 with a generic body.
	pub async fn read_password(&self, secret_name: &str, key: &str) -> Result<String> {
		match self {
			Self::Kube { client, namespace } => {
				use k8s_openapi::api::core::v1::Secret;
				use kube::Api;

				let api: Api<Secret> = Api::namespaced(client.clone(), namespace);
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
			Self::Memory(store) => store
				.lock()
				.unwrap()
				.get(secret_name)
				.and_then(|m| m.get(key))
				.cloned()
				.ok_or_else(|| AppError::Upstream(format!("secret get failed: {secret_name}"))),
		}
	}

	/// Create the named Secret holding `value` under `key` (fails if it already
	/// exists). Used by the private-server onboarding path.
	pub async fn create_password(&self, secret_name: &str, key: &str, value: &str) -> Result<()> {
		match self {
			Self::Kube { client, namespace } => {
				use k8s_openapi::api::core::v1::Secret;
				use kube::{Api, api::PostParams};

				let api: Api<Secret> = Api::namespaced(client.clone(), namespace);
				api.create(
					&PostParams::default(),
					&secret_object(secret_name, key, value),
				)
				.await
				.map_err(|e| AppError::Upstream(format!("secret create failed: {e}")))?;
				Ok(())
			}
			Self::Memory(store) => {
				store
					.lock()
					.unwrap()
					.entry(secret_name.to_string())
					.or_default()
					.insert(key.to_string(), value.to_string());
				Ok(())
			}
		}
	}

	/// Create-or-replace the named Secret's `key` with `value` (server-side
	/// apply). Used by the jobs rotation loop to publish a rotated passphrase
	/// over the existing Secret.
	pub async fn put_password(&self, secret_name: &str, key: &str, value: &str) -> Result<()> {
		match self {
			Self::Kube { client, namespace } => {
				use k8s_openapi::api::core::v1::Secret;
				use kube::{
					Api,
					api::{Patch, PatchParams},
				};

				let api: Api<Secret> = Api::namespaced(client.clone(), namespace);
				api.patch(
					secret_name,
					&PatchParams::apply("canopy-backups").force(),
					&Patch::Apply(secret_object(secret_name, key, value)),
				)
				.await
				.map_err(|e| AppError::Upstream(format!("secret apply failed: {e}")))?;
				Ok(())
			}
			Self::Memory(store) => {
				store
					.lock()
					.unwrap()
					.entry(secret_name.to_string())
					.or_default()
					.insert(key.to_string(), value.to_string());
				Ok(())
			}
		}
	}
}

/// Build a `Secret` carrying `value` under `key` named `secret_name`.
fn secret_object(secret_name: &str, key: &str, value: &str) -> k8s_openapi::api::core::v1::Secret {
	use k8s_openapi::api::core::v1::Secret;
	use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

	Secret {
		metadata: ObjectMeta {
			name: Some(secret_name.to_string()),
			..Default::default()
		},
		string_data: Some(BTreeMap::from([(key.to_string(), value.to_string())])),
		..Default::default()
	}
}

/// Namespace the repo-password Secrets live in: `POD_NAMESPACE` (inject via the
/// downward API), defaulting to `canopy`.
fn namespace_from_env() -> String {
	std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "canopy".to_string())
}
