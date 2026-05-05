use std::{net::SocketAddr, sync::Arc};

use miette::{Context, IntoDiagnostic, Result};
use quinn::Endpoint;

use crate::{keys, tls};

/// Bind a fresh frond-server endpoint on `addr`.
///
/// Generates an ephemeral Ed25519 keypair, builds the rustls + quinn config,
/// and returns the listening endpoint. The caller drives the accept loop.
pub fn bind(addr: SocketAddr) -> Result<Endpoint> {
	let key = keys::generate_ephemeral();
	let spki = keys::spki_der(&key);
	let fp = keys::fingerprint(&spki);
	tracing::info!(server_fingerprint = %fp, "frond-server identity (ephemeral; TODO: persist)");

	let tls = tls::build_server_config(&key, spki)
		.map_err(|e| miette::miette!("building TLS config: {e}"))?;
	let quic = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
		.into_diagnostic()
		.wrap_err("converting rustls config to quinn QuicServerConfig")?;

	let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic));
	Endpoint::server(server_config, addr)
		.into_diagnostic()
		.wrap_err_with(|| format!("binding QUIC endpoint on {addr}"))
}

/// Run the accept loop until the endpoint is closed.
///
/// Phase 2 stub: each accepted connection is logged and immediately closed.
/// Real stream handling lands in a later phase.
pub async fn accept_loop(endpoint: Endpoint) {
	while let Some(incoming) = endpoint.accept().await {
		tokio::spawn(handle_incoming(incoming));
	}
}

async fn handle_incoming(incoming: quinn::Incoming) {
	let peer = incoming.remote_address();
	match incoming.await {
		Ok(conn) => {
			let alpn = conn
				.handshake_data()
				.and_then(|d| {
					d.downcast::<quinn::crypto::rustls::HandshakeData>()
						.ok()
						.and_then(|d| d.protocol.clone())
				})
				.map(|p| String::from_utf8_lossy(&p).into_owned())
				.unwrap_or_default();
			tracing::info!(%peer, %alpn, "accepted connection (Phase 2 stub: closing)");
			conn.close(0u32.into(), b"phase 2 stub");
		}
		Err(e) => tracing::warn!(%peer, "incoming connection failed: {e}"),
	}
}
