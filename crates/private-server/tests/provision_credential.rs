//! Tests for operator-provisioned device credentials (spec DPK): Canopy mints
//! the keypair, stores only the public key, and returns the private key once
//! as a passphrase-encrypted age blob.

use algae_cli::passphrases::{Passphrase, SecretString};
use base64::Engine as _;
use commons_tests::server::{run, spki_from_key_pem};
use serde_json::{Value, json};

/// Decrypt an age/scrypt blob (as returned base64) with `passphrase`.
async fn reveal(key_age_base64: &str, passphrase: &str) -> String {
	let ciphertext = base64::engine::general_purpose::STANDARD
		.decode(key_age_base64)
		.expect("base64 decode");
	let mut out: Vec<u8> = Vec::new();
	algae_cli::streams::decrypt_stream(
		futures::io::Cursor::new(ciphertext),
		&mut out,
		Box::new(Passphrase::new(SecretString::from(passphrase.to_owned()))),
	)
	.await
	.expect("decrypt");
	String::from_utf8(out).expect("utf8 pem")
}

/// Decode the DER SPKI out of a "BEGIN PUBLIC KEY" PEM as returned by the
/// device key list (`pem_data`).
fn spki_from_public_pem(pem: &str) -> Vec<u8> {
	let body: String = pem
		.lines()
		.filter(|l| !l.starts_with("-----"))
		.collect::<String>();
	base64::engine::general_purpose::STANDARD
		.decode(body)
		.expect("decode spki")
}

#[tokio::test(flavor = "multi_thread")]
async fn provisions_new_device_at_role_with_revealable_key() {
	run(async |_conn, _public, private| {
		let res: Value = private
			.post("/api/devices/provision_credential")
			.json(&json!({ "role": "releaser" }))
			.await
			.json();

		let device_id = res["device_id"].as_str().unwrap().to_owned();
		let key_id = res["key_id"].as_str().unwrap().to_owned();
		let passphrase = res["passphrase"].as_str().unwrap().to_owned();
		let key_age = res["key_age_base64"].as_str().unwrap().to_owned();
		assert!(
			res["filename"].as_str().unwrap().ends_with(".pem.age"),
			"filename should be a .pem.age download",
		);
		assert!(!passphrase.is_empty(), "passphrase returned");

		// The blob decrypts to a PKCS#8 PEM private key.
		let pem = reveal(&key_age, &passphrase).await;
		assert!(
			pem.contains("BEGIN PRIVATE KEY"),
			"revealed material is a PKCS#8 PEM, got: {pem:.40}",
		);

		// The device now exists at the chosen role with exactly one active key,
		// and that stored public key corresponds to the revealed private key —
		// byte-for-byte what the mTLS path would extract from a cert.
		let device: Value = private
			.post("/api/devices/get_device_by_id")
			.json(&json!({ "device_id": device_id }))
			.await
			.json();

		assert_eq!(device["device"]["role"], "releaser");
		let keys = device["keys"].as_array().unwrap();
		assert_eq!(keys.len(), 1, "exactly one active key");
		assert_eq!(keys[0]["id"].as_str().unwrap(), key_id);

		let stored_spki = spki_from_public_pem(keys[0]["pem_data"].as_str().unwrap());
		let derived_spki = spki_from_key_pem(&pem);
		assert_eq!(
			stored_spki, derived_spki,
			"stored public key must match the revealed private key's SPKI",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn provisions_onto_existing_device_and_updates_role() {
	run(async |_conn, _public, private| {
		// Create an initial releaser device.
		let first: Value = private
			.post("/api/devices/provision_credential")
			.json(&json!({ "role": "releaser", "key_name": "first" }))
			.await
			.json();
		let device_id = first["device_id"].as_str().unwrap().to_owned();

		// Provision a second credential onto it, changing the role.
		let second: Value = private
			.post("/api/devices/provision_credential")
			.json(&json!({
				"role": "backup-restore",
				"device_id": device_id,
				"key_name": "second",
			}))
			.await
			.json();
		assert_eq!(second["device_id"].as_str().unwrap(), device_id);

		let device: Value = private
			.post("/api/devices/get_device_by_id")
			.json(&json!({ "device_id": device_id }))
			.await
			.json();
		assert_eq!(
			device["device"]["role"], "backup-restore",
			"role updated to match the new provisioning",
		);
		let keys = device["keys"].as_array().unwrap();
		assert_eq!(keys.len(), 2, "prior key kept alongside the new one");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_untrusted_role() {
	run(async |_conn, _public, private| {
		private
			.post("/api/devices/provision_credential")
			.json(&json!({ "role": "untrusted" }))
			.await
			.assert_status_bad_request();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_device_is_not_found() {
	run(async |_conn, _public, private| {
		private
			.post("/api/devices/provision_credential")
			.json(&json!({
				"role": "server",
				"device_id": "00000000-0000-0000-0000-000000000000",
			}))
			.await
			.assert_status_not_found();
	})
	.await;
}
