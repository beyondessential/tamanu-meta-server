use std::time::Duration;

use axum::{Json, extract::State, http::HeaderMap};
use axum_client_ip::ClientIp;
use base64::Engine;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::{mtls, pop};

use crate::ratelimit::RateLimiter;
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
use utoipa_axum::{router::OpenApiRouter, routes};
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

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicServer {
	pub name: String,
	pub host: UrlField,
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

#[utoipa::path(
	get,
	path = "/",
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct BeginArgs {
	pub server_id: Uuid,
	pub token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BeginResponse {
	/// Base64 (standard) of the 32-byte challenge nonce.
	pub nonce: String,
	/// True iff the server requires the TLS exporter (`EKM`) be folded into the
	/// signed transcript (channel binding). The client must then append it.
	pub channel_binding_required: bool,
}

/// Read the presented client cert's public key, or fail opaquely.
fn presented_key(headers: &HeaderMap) -> Result<Vec<u8>> {
	mtls::spki_from_headers(headers)
		.map_err(|_| AppError::EnrollmentFailed)?
		.ok_or(AppError::EnrollmentFailed)
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
	ClientIp(ip): ClientIp,
	headers: HeaderMap,
	Json(args): Json<BeginArgs>,
) -> Result<Json<BeginResponse>> {
	enforce_rate_limit(&rl, ip, args.server_id)?;
	let mut db = db.get().await?;
	let spki = presented_key(&headers)?;

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
		channel_binding_required: ekm_header_name().is_some(),
	}))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteArgs {
	pub server_id: Uuid,
	/// Base64 (standard) of the nonce returned by `begin`.
	pub nonce: String,
	/// Base64 (standard) of the ASN.1 DER ECDSA-P256-SHA256 signature over the
	/// transcript `nonce ‖ server_id ‖ spki [‖ ekm]`.
	pub signature: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CompleteResponse {
	pub server_id: Uuid,
	pub device_id: Uuid,
}

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
	ClientIp(ip): ClientIp,
	headers: HeaderMap,
	Json(args): Json<CompleteArgs>,
) -> Result<Json<CompleteResponse>> {
	enforce_rate_limit(&rl, ip, args.server_id)?;
	let mut db = db.get().await?;
	let spki = presented_key(&headers)?;

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
	if let Some(header_name) = ekm_header_name() {
		// Channel binding required: fold in the proxy-provided TLS exporter.
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
			let server = Server::get_by_id(conn, args.server_id).await?;
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
						// The old device kept working until now; release it (so it
						// can't authenticate as this server) and bind the new one.
						Device::untrust(conn, existing_id).await?;
						Device::deactivate_keys(conn, existing_id).await?;
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
