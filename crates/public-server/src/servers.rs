use axum::{Json, extract::State, http::HeaderMap};
use base64::Engine;
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::device_auth::{mtls, pop};
use commons_types::server::{kind::ServerKind, rank::ServerRank};
use database::{
	Db, devices::Device, server_enrollment_challenges::ServerEnrollmentChallenge,
	server_enrollment_tokens::ServerEnrollmentToken, servers::Server, url_field::UrlField,
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
			s.public_name.map(|name| PublicServer {
				name,
				host: s.host,
				rank: s.rank,
			})
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
	headers: HeaderMap,
	Json(args): Json<BeginArgs>,
) -> Result<Json<BeginResponse>> {
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
	headers: HeaderMap,
	Json(args): Json<CompleteArgs>,
) -> Result<Json<CompleteResponse>> {
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
	pop::verify_pop(&spki, &transcript, &signature)?;

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
					return Err(AppError::EnrollmentFailed);
				}
			}

			let device_id = if let Some(device_id) = server.device_id {
				// Tailscale-precreated device: attach the mTLS key (refuses if
				// it already carries a different active key).
				Device::add_key(conn, device_id, spki.clone()).await?;
				device_id
			} else {
				// Reuse the box's prior device row (key may be inactive after a
				// previous archival) or create a fresh one. Never merge.
				let device_id = match Device::from_key_any_state(conn, &spki).await? {
					Some(d) => {
						Device::add_key(conn, d.id, spki.clone()).await?;
						d.id
					}
					None => Device::create(conn, spki.clone()).await?.id,
				};
				Server::bind_device(conn, args.server_id, device_id).await?;
				device_id
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
