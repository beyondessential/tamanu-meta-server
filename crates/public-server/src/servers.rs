use std::time::Duration;

use axum::{Json, extract::State, http::HeaderMap};
use axum_client_ip::ClientIp;
use base64::Engine;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::{mtls, pop};
use commons_servers::tailnet_directory::TailnetDirectory;

use crate::ratelimit::RateLimiter;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_types::server::{kind::ServerKind, rank::ServerRank};
use database::{
	Db,
	devices::{Device, DeviceKey},
	server_enrollment_challenges::ServerEnrollmentChallenge,
	server_enrollment_tokens::ServerEnrollmentToken,
	servers::Server,
	url_field::UrlField,
};
use diesel_async::AsyncConnection;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

/// Env var naming the request header the terminating proxy populates with the
/// TLS exporter (RFC 9266 channel binding). When set, enrollment requires
/// channel binding; when unset, application-layer proof-of-possession runs
/// alone.
const EKM_HEADER_ENV: &str = "CANOPY_ENROLL_EKM_HEADER";

/// Challenge lifetime: short, since the device fetches it and immediately signs.
const CHALLENGE_TTL: jiff::SignedDuration = jiff::SignedDuration::from_mins(5);

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(register_begin))
		.routes(routes!(register_complete))
}

/// A publicly-listed central server that a client can connect to.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicServer {
	/// Public-facing display name of the server.
	pub name: String,
	/// The server's reachable base URL.
	pub host: UrlField,
	/// The server's environment tier (production, clone, demo, test, or
	/// dev), if set. Used to order the listing and to let clients label
	/// non-production entries.
	pub rank: Option<ServerRank>,
}

fn rank_order(rank: &Option<ServerRank>) -> u32 {
	match rank {
		Some(ServerRank::Production) => 0,
		Some(ServerRank::Clone) => 1,
		Some(ServerRank::Demo) => 2,
		Some(ServerRank::Test) => 3,
		Some(ServerRank::Dev) => 4,
		_ => 5,
	}
}

/// List publicly-listed central servers.
///
/// Returns every central server that has both a public display name and a
/// reachable host configured, ordered by environment tier (production
/// first, then clone, demo, test, dev) and then by name. Used by clients
/// to let a user pick which server to connect to.
#[utoipa::path(
	get,
	path = "/",
	operation_id = "list_servers",
	tag = "servers",
	responses(
		(status = 200, description = "Publicly-listed central servers, ordered by rank then name.", body = Vec<PublicServer>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn list(State(db): State<Db>) -> Result<Json<Vec<PublicServer>>> {
	let mut db = db.get().await?;
	let mut servers = Server::list_by_kind(&mut db, ServerKind::Central, 0, None)
		.await?
		.into_iter()
		.filter_map(|s| {
			// Only list servers that have both a public name and a URL — the
			// mobile app needs a reachable host.
			match (s.public_name, s.host) {
				(Some(name), Some(host)) => Some(PublicServer {
					name,
					host,
					rank: s.rank,
				}),
				_ => None,
			}
		})
		.collect::<Vec<_>>();

	servers.sort_by(|a, b| {
		rank_order(&a.rank)
			.cmp(&rank_order(&b.rank))
			.then_with(|| a.name.cmp(&b.name))
	});

	Ok(Json(servers))
}

/// Request to start device enrollment against a server.
#[derive(Debug, Deserialize, ToSchema)]
pub struct BeginArgs {
	/// ID of the server to enroll against.
	pub server_id: Uuid,
	/// The enrollment token issued by an operator for this server.
	pub token: String,
	/// Base64-standard-encoded DER SubjectPublicKeyInfo (SPKI) of the
	/// device's public key. Only required when enrolling over a transport
	/// with no client certificate to read the key from (e.g. over
	/// Tailscale); omit it when enrolling over mTLS, where the key is
	/// taken from the presented client certificate. The returned challenge
	/// is bound to this key, so the same value must be supplied again when
	/// completing enrollment.
	#[serde(default)]
	pub spki: Option<String>,
}

/// A freshly-issued enrollment challenge to sign and return.
#[derive(Debug, Serialize, ToSchema)]
pub struct BeginResponse {
	/// Base64-standard-encoded 32-byte challenge nonce. Sign it, together
	/// with the server ID, device public key, and channel-binding data if
	/// required, and submit the signature when completing enrollment.
	pub nonce: String,
	/// True if the server requires channel-binding data (the connection's
	/// TLS exported keying material) to be folded into the signed
	/// transcript. Only relevant when enrolling over mTLS; it never
	/// applies on a transport without TLS channel binding.
	pub channel_binding_required: bool,
}

/// Resolve the device's SPKI for an enrollment request. The source is fixed by
/// the transport, never chosen by precedence:
///
/// - **internet mTLS mount** (`tailnet` false): from the client-cert header the
///   terminating mTLS proxy sets. The request body's `spki` is ignored, so the
///   cert can never be skipped via a body field.
/// - **tailnet `/public` mount** (`tailnet` true): from the request body
///   (base64-standard DER). `tailscale serve` can't do client-cert mTLS, so the
///   `mtls-certificate` header is *not* set by a trusted terminator here — it is
///   attacker-controllable and therefore ignored.
fn resolve_spki(headers: &HeaderMap, body_spki: Option<&str>, tailnet: bool) -> Result<Vec<u8>> {
	if tailnet {
		let b64 = body_spki.ok_or(AppError::EnrollmentFailed)?;
		base64::engine::general_purpose::STANDARD
			.decode(b64)
			.map_err(|_| AppError::EnrollmentFailed)
	} else {
		mtls::spki_from_headers(headers)
			.map_err(|_| AppError::EnrollmentFailed)?
			.ok_or(AppError::EnrollmentFailed)
	}
}

/// The configured EKM header name, if channel binding is enabled.
fn ekm_header_name() -> Option<String> {
	std::env::var(EKM_HEADER_ENV).ok().filter(|s| !s.is_empty())
}

/// Enrollment rate-limit budgets within a 1-minute window. Enrollment is a
/// rare, human-paced operation, so these are generous for legitimate use but
/// blunt a token-guesser / griefer hammering the endpoints.
const RL_WINDOW: Duration = Duration::from_secs(60);
const RL_PER_IP: u32 = 60;
const RL_PER_SERVER: u32 = 20;

/// Enforce per-source and per-target rate limits on the enrollment endpoints.
/// A trip is logged (target `enrollment`) so a log-based alert can fire.
fn enforce_rate_limit(rl: &RateLimiter, ip: std::net::IpAddr, server_id: uuid::Uuid) -> Result<()> {
	let ip = ip.to_string();
	if !rl.check(&format!("ip:{ip}"), RL_PER_IP, RL_WINDOW) {
		tracing::warn!(target: "enrollment", ip, "enrollment rate limit exceeded (per-ip)");
		return Err(AppError::RateLimited);
	}
	if !rl.check(&format!("srv:{server_id}"), RL_PER_SERVER, RL_WINDOW) {
		tracing::warn!(target: "enrollment", %server_id, "enrollment rate limit exceeded (per-server)");
		return Err(AppError::RateLimited);
	}
	Ok(())
}

/// Start device enrollment against a server.
///
/// Validates the enrollment token against the given server and, if valid,
/// issues a short-lived (5 minute) signed challenge bound to the server
/// ID, the token, and the caller's public key. The device must sign this
/// challenge and submit it to the completion endpoint to finish
/// enrollment; the token itself is validated here but not yet consumed.
///
/// This endpoint is rate-limited per source IP and per target server; a
/// tripped limit returns 429. Any other failure — an unknown or deleted
/// server, or an invalid or expired token — is surfaced as a generic 403,
/// deliberately not distinguishing which check failed.
#[utoipa::path(
	post,
	path = "/register/begin",
	tag = "servers",
	request_body = BeginArgs,
	responses(
		(status = 200, body = BeginResponse),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn register_begin(
	State(db): State<Db>,
	State(rl): State<RateLimiter>,
	State(directory): State<Option<TailnetDirectory>>,
	ClientIp(ip): ClientIp,
	headers: HeaderMap,
	Json(args): Json<BeginArgs>,
) -> Result<Json<BeginResponse>> {
	enforce_rate_limit(&rl, ip, args.server_id)?;
	let mut db = db.get().await?;
	// `Some` only on the private-server's `/public` (tailnet) mount; `None` on
	// the internet binary — see `AppState::tailnet_directory`.
	let tailnet = directory.is_some();
	let spki = resolve_spki(&headers, args.spki.as_deref(), tailnet)?;

	// Server must exist and be live.
	let server = Server::get_by_id(&mut db, args.server_id)
		.await
		.map_err(|_| AppError::EnrollmentFailed)?;
	if server.deleted_at.is_some() {
		return Err(AppError::EnrollmentFailed);
	}

	// Validate (do not consume) the token, then issue a challenge bound to it
	// and the presented key.
	let token = ServerEnrollmentToken::find_active(&mut db, args.server_id, &args.token).await?;
	let nonce = ServerEnrollmentChallenge::create(
		&mut db,
		args.server_id,
		&token.token_hash,
		&spki,
		CHALLENGE_TTL,
	)
	.await?;

	Ok(Json(BeginResponse {
		nonce: base64::engine::general_purpose::STANDARD.encode(&nonce),
		// Channel binding is an mTLS-path concept (the TLS exporter comes from
		// the terminating proxy); the tailnet transport has no such exporter.
		channel_binding_required: !tailnet && ekm_header_name().is_some(),
	}))
}

/// Request to complete device enrollment by presenting a signed
/// challenge obtained from the start-enrollment endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteArgs {
	/// ID of the server being enrolled against. Must match the value used
	/// when starting enrollment.
	pub server_id: Uuid,
	/// Base64-standard-encoded challenge nonce returned when enrollment
	/// was started.
	pub nonce: String,
	/// Base64-standard-encoded ASN.1 DER ECDSA (P-256, SHA-256) signature
	/// over the challenge transcript, proving possession of the device's
	/// private key. The transcript is the byte concatenation of: the raw
	/// challenge nonce, the raw 16 bytes of the server ID, the DER SPKI of
	/// the device public key, and — only when channel binding was flagged
	/// as required — the connection's TLS exported keying material.
	pub signature: String,
	/// Base64-standard-encoded DER SPKI of the device's public key. Only
	/// required when enrolling over a transport with no client
	/// certificate to read the key from; must match the key used when
	/// enrollment was started.
	#[serde(default)]
	pub spki: Option<String>,
}

/// Result of a successful enrollment.
#[derive(Debug, Serialize, ToSchema)]
pub struct CompleteResponse {
	/// The server the device is now enrolled against.
	pub server_id: Uuid,
	/// The device identity created or reused for this enrollment. The
	/// device authenticates as this ID from now on.
	pub device_id: Uuid,
}

/// Complete device enrollment by presenting a signed challenge.
///
/// Verifies the signature over the challenge transcript using the public
/// key supplied here, then binds the device to the server: an existing
/// device re-enrolling with the same key is reused as-is; a device
/// re-enrolling with a different key replaces the server's previous
/// device (revoking that device's access); otherwise a new device
/// identity is created. On success the device is granted the server
/// role, the enrollment token is consumed, and the server is marked as
/// registered.
///
/// Enrollment is refused if the presented public key is already bound to
/// a different live server. Like the start-enrollment endpoint, this one
/// is rate-limited per source IP and per target server (429 on a tripped
/// limit) and reports every other kind of failure as a generic 403.
#[utoipa::path(
	post,
	path = "/register/complete",
	tag = "servers",
	request_body = CompleteArgs,
	responses(
		(status = 200, body = CompleteResponse),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn register_complete(
	State(db): State<Db>,
	State(rl): State<RateLimiter>,
	State(directory): State<Option<TailnetDirectory>>,
	ClientIp(ip): ClientIp,
	headers: HeaderMap,
	Json(args): Json<CompleteArgs>,
) -> Result<Json<CompleteResponse>> {
	enforce_rate_limit(&rl, ip, args.server_id)?;
	let mut db = db.get().await?;
	let tailnet = directory.is_some();
	let spki = resolve_spki(&headers, args.spki.as_deref(), tailnet)?;

	let nonce = base64::engine::general_purpose::STANDARD
		.decode(&args.nonce)
		.map_err(|_| AppError::EnrollmentFailed)?;
	let signature = base64::engine::general_purpose::STANDARD
		.decode(&args.signature)
		.map_err(|_| AppError::EnrollmentFailed)?;

	// Single-use take: also confirms the key matches the one used at `begin`.
	let challenge = ServerEnrollmentChallenge::take(&mut db, args.server_id, &nonce, &spki).await?;

	// Build the transcript and verify proof-of-possession.
	let mut transcript = nonce.clone();
	transcript.extend_from_slice(args.server_id.as_bytes());
	transcript.extend_from_slice(&spki);
	if !tailnet && let Some(header_name) = ekm_header_name() {
		// Channel binding required: fold in the proxy-provided TLS exporter.
		// mTLS path only — the tailnet transport has no TLS exporter.
		let ekm = headers
			.get(&header_name)
			.and_then(|v| v.to_str().ok())
			.and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
			.ok_or(AppError::EnrollmentFailed)?;
		transcript.extend_from_slice(&ekm);
	}
	if let Err(e) = pop::verify_pop(&spki, &transcript, &signature) {
		// A valid challenge was presented but the signature doesn't match the
		// cert's key — either a misbehaving client or an attacker who holds a
		// token but not the private key. Worth surfacing.
		tracing::warn!(
			target: "enrollment",
			server_id = %args.server_id,
			"enrollment proof-of-possession signature verification failed",
		);
		return Err(e);
	}

	// Bind + promote + burn the token, atomically. The challenge is already
	// spent; a failure here strands the token (operator reissues) but never
	// double-binds.
	let device_id = db
		.transaction::<_, AppError, _>(async |conn| {
			// Lock the server row so a concurrent archival can't slip in between
			// this check and the bind/burn below (it also locks FOR UPDATE).
			let server = Server::get_by_id_for_update(conn, args.server_id).await?;
			if server.deleted_at.is_some() {
				return Err(AppError::EnrollmentFailed);
			}

			// Refuse to graft this key onto an identity already serving a
			// different live server.
			if let Some(existing) = Device::from_key(conn, &spki).await? {
				if Server::live_by_device_id(conn, existing.id)
					.await?
					.iter()
					.any(|s| s.id != args.server_id)
				{
					tracing::warn!(
						target: "enrollment",
						server_id = %args.server_id,
						device_id = %existing.id,
						"enrollment rejected: presented key is already bound to another live server",
					);
					return Err(AppError::EnrollmentFailed);
				}
			}

			// Resolve the device to bind. Helper: reuse the box's prior device
			// row (key may be inactive after archival) or create a fresh one,
			// then bind it to the server. Never merges.
			async fn bind_fresh_device(
				conn: &mut diesel_async::AsyncPgConnection,
				server_id: uuid::Uuid,
				spki: &[u8],
			) -> Result<uuid::Uuid> {
				let device_id = match Device::from_key_any_state(conn, spki).await? {
					Some(d) => {
						Device::add_key(conn, d.id, spki.to_vec()).await?;
						d.id
					}
					None => Device::create(conn, spki.to_vec()).await?.id,
				};
				Server::bind_device(conn, server_id, device_id).await?;
				Ok(device_id)
			}

			let device_id = match server.device_id {
				Some(existing_id) => {
					let active = DeviceKey::find_by_device(conn, existing_id).await?;
					if active.iter().any(|k| k.key_data == spki) {
						// Same box re-running — idempotent.
						existing_id
					} else if active.is_empty() {
						// Tailscale-precreated device gaining its first mTLS key.
						Device::add_key(conn, existing_id, spki.clone()).await?;
						existing_id
					} else {
						// Re-enrollment with a *different* box: replace the device.
						// The old device kept working until now; revoke its access
						// (so it can't authenticate as this server) and bind the new
						// one.
						Device::revoke(conn, existing_id).await?;
						bind_fresh_device(conn, args.server_id, &spki).await?
					}
				}
				None => bind_fresh_device(conn, args.server_id, &spki).await?,
			};

			Device::trust(conn, device_id, commons_types::device::DeviceRole::Server).await?;
			ServerEnrollmentToken::consume(conn, args.server_id, &challenge.token_hash).await?;
			Server::mark_registered(conn, args.server_id).await?;
			Ok(device_id)
		})
		.await?;

	Ok(Json(CompleteResponse {
		server_id: args.server_id,
		device_id,
	}))
}
