use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use sha2::{Digest, Sha256};

/// Generate a fresh Ed25519 keypair in memory.
///
/// TODO: persist this in the database and load on startup so that the
/// server's SPKI is stable across restarts.
pub fn generate_ephemeral() -> SigningKey {
	SigningKey::generate(&mut OsRng)
}

/// Build the SubjectPublicKeyInfo (SPKI) DER encoding for an Ed25519 key.
///
/// ```text
/// SEQUENCE {
///   SEQUENCE { OID 1.3.101.112 }      -- Ed25519
///   BIT STRING { 0x00 || 32-byte key }
/// }
/// ```
pub fn spki_der(key: &SigningKey) -> Vec<u8> {
	const PREFIX: [u8; 12] = [
		0x30, 0x2a, // SEQUENCE 42 bytes
		0x30, 0x05, // SEQUENCE 5 bytes
		0x06, 0x03, 0x2b, 0x65, 0x70, // OID 1.3.101.112
		0x03, 0x21, 0x00, // BIT STRING 33 bytes, 0 unused bits
	];
	let mut out = Vec::with_capacity(44);
	out.extend_from_slice(&PREFIX);
	out.extend_from_slice(key.verifying_key().as_bytes());
	out
}

/// SHA-256 fingerprint of a byte slice as lowercase hex.
pub fn fingerprint(bytes: &[u8]) -> String {
	Sha256::digest(bytes)
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect()
}
