use std::sync::Arc;

use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey};
use rustls::{
	DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
	client::danger::HandshakeSignatureValid,
	server::{
		AlwaysResolvesServerRawPublicKeys,
		danger::{ClientCertVerified, ClientCertVerifier},
	},
	sign::CertifiedKey,
};
use rustls_pki_types::{
	CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, SubjectPublicKeyInfoDer, UnixTime,
};

use crate::ALPN;

/// Build the rustls server config used by frond-server.
///
/// `key` is the server's signing key (paired with `spki`, its DER-encoded
/// SubjectPublicKeyInfo). The resulting config:
///
/// - Presents `spki` as a raw public key (RFC 7250) instead of an X.509 cert.
/// - Requires clients to do the same.
/// - Negotiates only the `bes.canopy/1` ALPN.
/// - Uses [`PermissiveClientVerifier`] so any well-formed client RPK is
///   accepted. Phase 4 swaps this for an allowlist verifier.
pub fn build_server_config(
	key: &SigningKey,
	spki: Vec<u8>,
) -> Result<ServerConfig, Box<dyn std::error::Error + Send + Sync>> {
	let pkcs8 = key.to_pkcs8_der()?;
	let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8.as_bytes().to_vec()));
	let signing_key = rustls::crypto::ring::sign::any_supported_type(&private_key)?;

	let cert = CertificateDer::from(spki);
	let certified_key = Arc::new(CertifiedKey::new(vec![cert], signing_key));
	let resolver = Arc::new(AlwaysResolvesServerRawPublicKeys::new(certified_key));

	let client_verifier: Arc<dyn ClientCertVerifier> = Arc::new(PermissiveClientVerifier);

	let mut tls = ServerConfig::builder()
		.with_client_cert_verifier(client_verifier)
		.with_cert_resolver(resolver);

	tls.alpn_protocols = vec![ALPN.to_vec()];
	Ok(tls)
}

/// Phase 2 stand-in: accepts any well-formed client raw public key without
/// allowlist or DB lookup. Phase 4 of `docs/plans/frond-server.md` replaces
/// this with a verifier that pins a specific SPKI from `identity.pub.pem`,
/// and a later phase swaps in a `device_keys` lookup.
#[derive(Debug)]
pub struct PermissiveClientVerifier;

impl ClientCertVerifier for PermissiveClientVerifier {
	fn root_hint_subjects(&self) -> &[DistinguishedName] {
		&[]
	}

	fn verify_client_cert(
		&self,
		end_entity: &CertificateDer<'_>,
		_intermediates: &[CertificateDer<'_>],
		_now: UnixTime,
	) -> Result<ClientCertVerified, rustls::Error> {
		let fp = crate::keys::fingerprint(end_entity.as_ref());
		tracing::debug!(client_fingerprint = %fp, "permissive verifier accepted client");
		Ok(ClientCertVerified::assertion())
	}

	fn verify_tls12_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		rustls::crypto::verify_tls12_signature(
			message,
			cert,
			dss,
			&rustls::crypto::ring::default_provider().signature_verification_algorithms,
		)
	}

	fn verify_tls13_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		rustls::crypto::verify_tls13_signature_with_raw_key(
			message,
			&SubjectPublicKeyInfoDer::from(cert.as_ref()),
			dss,
			&rustls::crypto::ring::default_provider().signature_verification_algorithms,
		)
	}

	fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
		rustls::crypto::ring::default_provider()
			.signature_verification_algorithms
			.supported_schemes()
	}

	fn requires_raw_public_keys(&self) -> bool {
		true
	}
}
