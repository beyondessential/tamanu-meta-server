//! Shared `age` helper for the recovery secret vault.
//!
//! Canopy owns every repo passphrase with no human copy, so it backs up its
//! recovery-critical state to an object-locked S3 bucket — but encrypted with **`age`
//! asymmetric** crypto to one or more recipient public keys whose private keys
//! Canopy never holds. Canopy can *write* the vault but cannot *read* it back, so
//! a full Canopy compromise can't disclose the historical secrets.
//!
//! This module is the recipient/encryption half, shared by the jobs writer
//! ([`crate::backup_secrets`]-adjacent) and the private-server verification
//! ceremony. The S3 write + the operator-facing challenge/verify live in their
//! respective crates.
//!
//! Recipients come from `CANOPY_RECOVERY_VAULT_KEYS` (whitespace/comma-separated
//! `age1…` public keys). The "fingerprint" of a recipient is simply its public
//! `age1…` string — it's public and uniquely identifying, and lets the ceremony
//! detect when the recipient set has changed.
//!
//! ## bestool compatibility
//!
//! Output is **standard `age` v1** to `x25519` recipients — the exact same crate
//! (`age` 0.11.3) and format BES's `algae-cli` uses. So a vault object is
//! decryptable by **`bestool crypto decrypt`** (algae) and by plain `age`/`rage`,
//! and the recipient keys are whatever `bestool crypto keygen` emits. Recovery
//! uses one of the recipients' (offline) private keys with `bestool crypto
//! decrypt`. We don't depend on `algae-cli` directly: it's CLI/async-stream
//! shaped and single-recipient, whereas this is a small sync multi-recipient
//! helper — but the on-the-wire format is identical (asserted in the tests).

use std::str::FromStr;

use commons_errors::{AppError, Result};

/// Env var holding the whitespace/comma-separated `age1…` recipient public keys.
pub const RECIPIENTS_ENV: &str = "CANOPY_RECOVERY_VAULT_KEYS";

/// One or more `age` recipients Canopy encrypts the recovery vault to. Holds the parsed
/// keys plus their canonical `age1…` strings (used as stable fingerprints).
#[derive(Clone)]
pub struct Recipients {
	keys: Vec<age::x25519::Recipient>,
	fingerprints: Vec<String>,
}

impl std::fmt::Debug for Recipients {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Recipients")
			.field("fingerprints", &self.fingerprints)
			.finish()
	}
}

impl Recipients {
	/// Parse from a whitespace/comma-separated list of `age1…` keys. Errors if any
	/// token fails to parse or the list is empty.
	pub fn parse(list: &str) -> Result<Self> {
		let mut keys = Vec::new();
		let mut fingerprints = Vec::new();
		for tok in list.split([',', ' ', '\t', '\n', '\r']) {
			let tok = tok.trim();
			if tok.is_empty() {
				continue;
			}
			let key = age::x25519::Recipient::from_str(tok)
				.map_err(|e| AppError::BadRequest(format!("invalid age recipient {tok:?}: {e}")))?;
			fingerprints.push(tok.to_string());
			keys.push(key);
		}
		if keys.is_empty() {
			return Err(AppError::BadRequest("no age recipients configured".into()));
		}
		Ok(Self { keys, fingerprints })
	}

	/// Parse from `CANOPY_RECOVERY_VAULT_KEYS`. `Ok(None)` if unset/blank;
	/// `Err` if set but unparseable.
	pub fn from_env() -> Result<Option<Self>> {
		match std::env::var(RECIPIENTS_ENV) {
			Err(_) => Ok(None),
			Ok(v) if v.trim().is_empty() => Ok(None),
			Ok(v) => Self::parse(&v).map(Some),
		}
	}

	/// The recipients' `age1…` strings — stable, public identifiers the ceremony
	/// compares to detect a changed recipient set.
	pub fn fingerprints(&self) -> &[String] {
		&self.fingerprints
	}

	pub fn len(&self) -> usize {
		self.keys.len()
	}

	pub fn is_empty(&self) -> bool {
		self.keys.is_empty()
	}

	/// Encrypt `plaintext` to all recipients (binary `age` format). Any one of the
	/// recipients' private keys can later decrypt it.
	pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
		use std::io::Write;

		let encryptor =
			age::Encryptor::with_recipients(self.keys.iter().map(|r| r as &dyn age::Recipient))
				.map_err(|e| AppError::Upstream(format!("age encryptor: {e}")))?;
		let mut out = Vec::new();
		let mut writer = encryptor
			.wrap_output(&mut out)
			.map_err(|e| AppError::Upstream(format!("age wrap_output: {e}")))?;
		writer
			.write_all(plaintext)
			.map_err(|e| AppError::Upstream(format!("age write: {e}")))?;
		writer
			.finish()
			.map_err(|e| AppError::Upstream(format!("age finish: {e}")))?;
		Ok(out)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Read;

	#[test]
	fn parse_rejects_garbage_and_empty() {
		assert!(Recipients::parse("not-an-age-key").is_err());
		assert!(Recipients::parse("   ").is_err());
	}

	#[test]
	fn encrypt_roundtrips_with_any_recipient_key() {
		// Two recipients; either private key must decrypt the same ciphertext.
		let id_a = age::x25519::Identity::generate();
		let id_b = age::x25519::Identity::generate();
		let list = format!("{} {}", id_a.to_public(), id_b.to_public());

		let recipients = Recipients::parse(&list).unwrap();
		assert_eq!(recipients.len(), 2);
		assert_eq!(recipients.fingerprints().len(), 2);

		let plaintext = br#"{"hello":"recovery-vault"}"#;
		let ciphertext = recipients.encrypt(plaintext).unwrap();
		assert_ne!(&ciphertext[..], &plaintext[..]);
		// Standard age v1 framing → decryptable by `bestool crypto decrypt`
		// (algae-cli) and `age`/`rage`, not a bespoke envelope.
		assert!(ciphertext.starts_with(b"age-encryption.org/v1\n"));

		for id in [&id_a, &id_b] {
			let decryptor = age::Decryptor::new_buffered(&ciphertext[..]).unwrap();
			let mut reader = decryptor
				.decrypt(std::iter::once(id as &dyn age::Identity))
				.unwrap();
			let mut got = Vec::new();
			reader.read_to_end(&mut got).unwrap();
			assert_eq!(got, plaintext);
		}
	}
}
