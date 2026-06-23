//! Localhost container-credentials endpoint for kopia subprocesses.
//!
//! kopia (minio-go) can't use `credential_process`, but its IAM provider polls
//! an ECS-style **container-credentials endpoint** and self-refreshes. So we run
//! a tiny loopback HTTP server: each in-flight kopia op [`lease`](CredsServer::lease)s
//! a token mapped to its group's maintenance role, and the kopia subprocess is
//! pointed at the endpoint via `AWS_CONTAINER_CREDENTIALS_FULL_URI` +
//! `AWS_CONTAINER_AUTHORIZATION_TOKEN` (see [`super::kopia::KopiaEnv::apply`]).
//! minio-go re-polls before expiry; we mint each session with an in-process
//! [`AssumeRoleProvider`] whose base (the pod's `canopy-jobs` IRSA identity) is
//! auto-refreshed by the Rust SDK — so a run isn't capped at the 1h
//! chained-session limit.
//!
//! Verified against kopia v0.23.1 + minio-go v7.2.0 (FULL_URI/ECS path): GET to
//! the URI with a raw `Authorization: <token>` header; reply HTTP 200 + JSON
//! `{AccessKeyId, SecretAccessKey, Token, Expiration}` (`Token`, not
//! SessionToken; `Expiration` RFC3339); the host must be loopback (we bind
//! `127.0.0.1`). The kopia subprocess env must NOT carry the pod's
//! `AWS_WEB_IDENTITY_TOKEN_FILE`/static AWS creds/relative-URI — those precede
//! this path; `KopiaEnv::apply` scrubs them.

use std::{
	collections::HashMap,
	net::{Ipv4Addr, SocketAddr},
	sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use aws_config::{BehaviorVersion, sts::AssumeRoleProvider};
use aws_sdk_sts::config::{ProvideCredentials, Region, SharedCredentialsProvider};
use axum::{
	Json, Router,
	extract::State,
	http::{HeaderMap, StatusCode, header::AUTHORIZATION},
	routing::get,
};
use jiff::Timestamp;
use serde::Serialize;
use tokio::net::TcpListener;
use tracing::error;
use uuid::Uuid;

type Registry = Arc<Mutex<HashMap<String, SharedCredentialsProvider>>>;

/// Handle to the running loopback creds endpoint. Cheaply cloneable; shared by
/// the maintenance + inspection loops via the [`super::worker::Worker`].
#[derive(Clone)]
pub struct CredsServer {
	base_uri: String,
	sdk_config: aws_config::SdkConfig,
	registry: Registry,
}

impl CredsServer {
	/// Bind a loopback server on an ephemeral port and start serving in the
	/// background. The base credentials come from the pod's default chain (its
	/// `canopy-jobs` IRSA identity), which the SDK refreshes on its own.
	pub async fn start() -> Result<Self> {
		let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
		let registry: Registry = Arc::new(Mutex::new(HashMap::new()));

		let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
			.await
			.context("binding container-creds endpoint")?;
		let addr: SocketAddr = listener
			.local_addr()
			.context("reading container-creds endpoint address")?;
		// Loopback literal (not "localhost") — minio-go verifies the host
		// resolves only to loopback IPs.
		let base_uri = format!("http://127.0.0.1:{}", addr.port());

		let app = Router::new()
			.route("/creds", get(handler))
			.with_state(registry.clone());
		tokio::spawn(async move {
			if let Err(e) = axum::serve(listener, app).await {
				error!("container-creds endpoint exited: {e}");
			}
		});

		Ok(CredsServer {
			base_uri,
			sdk_config,
			registry,
		})
	}

	/// Lease a token bound to `role_arn` for the lifetime of the returned
	/// [`CredsLease`]. Point the kopia subprocess at [`CredsLease::uri`] with
	/// [`CredsLease::token`]; the mapping is dropped (deregistered) when the
	/// lease is dropped.
	pub async fn lease(&self, role_arn: &str, region: Option<&str>) -> Result<CredsLease> {
		let mut builder = AssumeRoleProvider::builder(role_arn)
			.session_name("canopy-maintenance")
			.configure(&self.sdk_config);
		if let Some(r) = region {
			builder = builder.region(Region::new(r.to_string()));
		}
		let provider = SharedCredentialsProvider::new(builder.build().await);

		let token = Uuid::new_v4().to_string();
		self.registry
			.lock()
			.unwrap()
			.insert(token.clone(), provider);

		Ok(CredsLease {
			uri: format!("{}/creds", self.base_uri),
			token,
			registry: self.registry.clone(),
		})
	}

	/// Assume `role_arn` and return its current **static** credentials, for the
	/// kopia subprocess's `AWS_*` env. kopia's S3 connector requires real keys
	/// (it can't use the loopback endpoint — its CLI demands `--access-key` at
	/// parse time), so short kopia ops (init / stats / inspection) take static
	/// assumed-role creds. These are ~1h-lived; long ops (maintenance) must use a
	/// refreshing sigv4 proxy instead, not this.
	pub async fn resolve(&self, role_arn: &str, region: Option<&str>) -> Result<ResolvedCreds> {
		let mut builder = AssumeRoleProvider::builder(role_arn)
			.session_name("canopy-kopia")
			.configure(&self.sdk_config);
		if let Some(r) = region {
			builder = builder.region(Region::new(r.to_string()));
		}
		let creds = builder
			.build()
			.await
			.provide_credentials()
			.await
			.context("assume role for kopia static credentials")?;
		Ok(ResolvedCreds {
			access_key_id: creds.access_key_id().to_string(),
			secret_access_key: creds.secret_access_key().to_string(),
			session_token: creds.session_token().unwrap_or_default().to_string(),
		})
	}
}

/// Static credentials resolved from an assumed role, for a kopia subprocess's
/// `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` env.
pub struct ResolvedCreds {
	pub access_key_id: String,
	pub secret_access_key: String,
	pub session_token: String,
}

/// An active credentials lease. Deregisters its token on drop, so a leaked token
/// stops working once the op finishes.
pub struct CredsLease {
	uri: String,
	token: String,
	registry: Registry,
}

impl CredsLease {
	/// The `AWS_CONTAINER_CREDENTIALS_FULL_URI` to set on the kopia subprocess.
	pub fn uri(&self) -> &str {
		&self.uri
	}

	/// The `AWS_CONTAINER_AUTHORIZATION_TOKEN` to set on the kopia subprocess.
	pub fn token(&self) -> &str {
		&self.token
	}
}

impl Drop for CredsLease {
	fn drop(&mut self) {
		if let Ok(mut reg) = self.registry.lock() {
			reg.remove(&self.token);
		}
	}
}

/// The container-credentials JSON minio-go expects (field names case-insensitive
/// via `encoding/json`; `Token` is the session token, not `SessionToken`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CredsResponse {
	access_key_id: String,
	secret_access_key: String,
	token: String,
	expiration: String,
}

async fn handler(
	State(registry): State<Registry>,
	headers: HeaderMap,
) -> Result<Json<CredsResponse>, StatusCode> {
	let token = headers
		.get(AUTHORIZATION)
		.and_then(|v| v.to_str().ok())
		.unwrap_or_default();
	// Clone the provider out of the lock; never hold the std mutex across await.
	let provider = registry.lock().unwrap().get(token).cloned();
	let Some(provider) = provider else {
		return Err(StatusCode::FORBIDDEN);
	};

	let creds = provider.provide_credentials().await.map_err(|e| {
		error!("assume-role for container-creds failed: {e}");
		StatusCode::BAD_GATEWAY
	})?;

	// minio-go re-polls at ~80% of the remaining lifetime. AssumeRole always sets
	// an expiry; fall back to a few minutes out only if it somehow didn't.
	let expiration = creds
		.expiry()
		.and_then(|st| Timestamp::try_from(st).ok())
		.unwrap_or_else(|| Timestamp::from_second(Timestamp::now().as_second() + 900).unwrap())
		.to_string();

	Ok(Json(CredsResponse {
		access_key_id: creds.access_key_id().to_string(),
		secret_access_key: creds.secret_access_key().to_string(),
		token: creds.session_token().unwrap_or_default().to_string(),
		expiration,
	}))
}

#[cfg(test)]
mod tests {
	use aws_sdk_sts::config::Credentials;
	use axum::http::HeaderValue;

	use super::*;

	fn registry_with(token: &str, creds: Credentials) -> Registry {
		let mut map = HashMap::new();
		map.insert(token.to_string(), SharedCredentialsProvider::new(creds));
		Arc::new(Mutex::new(map))
	}

	fn auth(token: &str) -> HeaderMap {
		let mut h = HeaderMap::new();
		h.insert(AUTHORIZATION, HeaderValue::from_str(token).unwrap());
		h
	}

	#[tokio::test]
	async fn valid_token_returns_container_creds_json() {
		let creds = Credentials::new("AKID", "SECRET", Some("SESSION".into()), None, "test");
		let reg = registry_with("tok-1", creds);

		let body = handler(State(reg), auth("tok-1")).await.expect("200").0;

		assert_eq!(body.access_key_id, "AKID");
		assert_eq!(body.secret_access_key, "SECRET");
		// minio-go expects the session token under `Token`, not `SessionToken`.
		assert_eq!(body.token, "SESSION");
		// Always present (RFC3339); falls back a few minutes out when the
		// provider sets no expiry, so minio-go never re-polls every request.
		assert!(body.expiration.ends_with('Z'), "{}", body.expiration);
	}

	#[tokio::test]
	async fn unknown_token_is_forbidden() {
		let reg: Registry = Arc::new(Mutex::new(HashMap::new()));
		let err = handler(State(reg), auth("nope")).await.unwrap_err();
		assert_eq!(err, StatusCode::FORBIDDEN);
	}

	#[tokio::test]
	async fn dropping_a_lease_deregisters_its_token() {
		let reg = registry_with("leased", Credentials::new("A", "S", None, None, "t"));
		let lease = CredsLease {
			uri: "http://127.0.0.1:0/creds".into(),
			token: "leased".into(),
			registry: reg.clone(),
		};
		assert!(reg.lock().unwrap().contains_key("leased"));
		drop(lease);
		assert!(!reg.lock().unwrap().contains_key("leased"));
	}
}
