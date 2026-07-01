//! Server-side minting of device keypairs for operator-provisioned
//! credentials (see spec DPK). Canopy generates the keypair, keeps only the
//! public [`GeneratedDeviceKey::spki_der`], and hands the private key back to
//! the operator once. The private PEM here is sensitive — never persist or log
//! it.

use std::fmt::Write as _;

use commons_errors::{AppError, Result};
use rcgen::{
	CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
	PKCS_ECDSA_P256_SHA256,
};
use x509_parser::prelude::*;

/// A freshly-minted device keypair.
pub struct GeneratedDeviceKey {
	/// PKCS#8 PEM of the private key. Sensitive: this is the material the
	/// operator downloads, and it must never be persisted or logged.
	pub private_key_pem: String,
	/// DER `SubjectPublicKeyInfo`, byte-identical to what the mTLS path
	/// extracts from a presented certificate (`subject_pki.raw`). This is what
	/// gets stored in `device_keys.key_data`.
	pub spki_der: Vec<u8>,
	/// Lowercase hex SHA-256 of `spki_der`, for operator correlation in the UI.
	pub fingerprint: String,
}

/// Generate a P-256 device keypair.
///
/// The SPKI is derived exactly as the auth path derives it — by self-signing a
/// throwaway certificate and reading `subject_pki.raw` — so the bytes stored
/// against the device are guaranteed to match what `spki_from_headers` would
/// extract from a certificate this key later presents.
pub fn generate_device_key() -> Result<GeneratedDeviceKey> {
	let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
		.map_err(|e| AppError::custom(format!("generating device key: {e}")))?;
	let private_key_pem = key.serialize_pem();

	let mut params = CertificateParams::default();
	params.is_ca = IsCa::NoCa;
	params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
	params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
	params.distinguished_name = DistinguishedName::new();
	let cert = params
		.self_signed(&key)
		.map_err(|e| AppError::custom(format!("self-signing device cert: {e}")))?;
	let cert_pem = cert.pem();

	let (_, pem) = parse_x509_pem(cert_pem.as_bytes())
		.map_err(|e| AppError::custom(format!("parsing device cert pem: {e}")))?;
	let (_, x509) = parse_x509_certificate(&pem.contents)
		.map_err(|e| AppError::custom(format!("parsing device cert: {e}")))?;
	let spki_der = x509.tbs_certificate.subject_pki.raw.to_vec();

	let digest = ring::digest::digest(&ring::digest::SHA256, &spki_der);
	let mut fingerprint = String::with_capacity(digest.as_ref().len() * 2);
	for b in digest.as_ref() {
		let _ = write!(fingerprint, "{b:02x}");
	}

	Ok(GeneratedDeviceKey {
		private_key_pem,
		spki_der,
		fingerprint,
	})
}
