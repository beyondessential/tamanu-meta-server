//! Tests for device key management: registering an externally-generated public
//! key, per-key disable/enable, and disable-all.

use base64::Engine as _;
use commons_tests::server::{make_certificate, run};
use serde_json::{Value, json};

/// Provision a device and return `(device_id, first_key_id)`.
async fn provision(private: &commons_tests::axum_test::TestServer) -> (String, String) {
	let dev: Value = private
		.post("/api/devices/provision_credential")
		.json(&json!({ "role": "server" }))
		.await
		.json();
	(
		dev["device_id"].as_str().unwrap().to_owned(),
		dev["key_id"].as_str().unwrap().to_owned(),
	)
}

fn public_key_b64() -> String {
	// A real DER SubjectPublicKeyInfo (P-256), base64'd — add_key accepts bare
	// base64 or PEM armor.
	let (spki, _cert) = make_certificate();
	base64::engine::general_purpose::STANDARD.encode(spki)
}

#[tokio::test(flavor = "multi_thread")]
async fn add_key_from_public_key() {
	run(async |_conn, _public, private| {
		let (device_id, _) = provision(&private).await;

		private
			.post("/api/devices/add_key")
			.json(&json!({
				"device_id": device_id,
				"public_key_pem": public_key_b64(),
				"name": "external",
			}))
			.await
			.assert_status_ok();

		let info: Value = private
			.post("/api/devices/get_device_by_id")
			.json(&json!({ "device_id": device_id }))
			.await
			.json();
		let keys = info["keys"].as_array().unwrap();
		assert_eq!(keys.len(), 2, "provisioned key + added key");
		assert!(
			keys.iter()
				.any(|k| k["name"] == "external" && k["is_active"] == true),
			"added key is present and active",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn add_key_rejects_non_spki() {
	run(async |_conn, _public, private| {
		let (device_id, _) = provision(&private).await;
		// Valid base64, but not a SubjectPublicKeyInfo.
		let garbage = base64::engine::general_purpose::STANDARD.encode(b"not a key");
		private
			.post("/api/devices/add_key")
			.json(&json!({ "device_id": device_id, "public_key_pem": garbage }))
			.await
			.assert_status_bad_request();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn disable_then_reenable_single_key() {
	run(async |_conn, _public, private| {
		let (device_id, key_id) = provision(&private).await;

		private
			.post("/api/devices/deactivate_key")
			.json(&json!({ "key_id": key_id }))
			.await
			.assert_status_ok();
		let info: Value = private
			.post("/api/devices/get_device_by_id")
			.json(&json!({ "device_id": device_id }))
			.await
			.json();
		let keys = info["keys"].as_array().unwrap();
		assert_eq!(keys.len(), 1, "disabled key still listed for history");
		assert_eq!(keys[0]["is_active"], false);

		private
			.post("/api/devices/reactivate_key")
			.json(&json!({ "key_id": key_id }))
			.await
			.assert_status_ok();
		let info: Value = private
			.post("/api/devices/get_device_by_id")
			.json(&json!({ "device_id": device_id }))
			.await
			.json();
		assert_eq!(info["keys"].as_array().unwrap()[0]["is_active"], true);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn disable_all_keys_keeps_rows_inactive() {
	run(async |_conn, _public, private| {
		let (device_id, _) = provision(&private).await;
		private
			.post("/api/devices/add_key")
			.json(&json!({ "device_id": device_id, "public_key_pem": public_key_b64() }))
			.await
			.assert_status_ok();

		private
			.post("/api/devices/disable_all_keys")
			.json(&json!({ "device_id": device_id }))
			.await
			.assert_status_ok();

		let info: Value = private
			.post("/api/devices/get_device_by_id")
			.json(&json!({ "device_id": device_id }))
			.await
			.json();
		let keys = info["keys"].as_array().unwrap();
		assert_eq!(keys.len(), 2, "keys kept for history");
		assert!(
			keys.iter().all(|k| k["is_active"] == false),
			"every key disabled",
		);
	})
	.await;
}
