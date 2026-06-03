//! Proof-of-possession verification for enrollment. The device signs a
//! server-issued challenge transcript with the private key behind the
//! certificate it presents; we verify that signature against the public key in
//! the presented `SubjectPublicKeyInfo`. This gives the application layer
//! cryptographic proof the caller holds the private key, independent of the
//! terminating proxy.
//!
//! Only ECDSA P-256 / SHA-256 (ASN.1 DER signatures) is supported, matching
//! what bestool generates. Every failure collapses to the opaque
//! `EnrollmentFailed` so the endpoint reveals nothing.

use commons_errors::{AppError, Result};
use ring::signature;
use x509_parser::prelude::*;
use x509_parser::public_key::PublicKey;

/// Verify an ECDSA P-256 / SHA-256 signature (ASN.1 DER) over `message` against
/// the public key contained in `spki_der` (a DER-encoded SubjectPublicKeyInfo).
pub fn verify_pop(spki_der: &[u8], message: &[u8], signature_bytes: &[u8]) -> Result<()> {
	let (_, spki) =
		SubjectPublicKeyInfo::from_der(spki_der).map_err(|_| AppError::EnrollmentFailed)?;

	let point = match spki.parsed().map_err(|_| AppError::EnrollmentFailed)? {
		PublicKey::EC(ec) => ec.data().to_vec(),
		_ => return Err(AppError::EnrollmentFailed),
	};

	signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, point)
		.verify(message, signature_bytes)
		.map_err(|_| AppError::EnrollmentFailed)
}
