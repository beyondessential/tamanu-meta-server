//! End-to-end tests for the operator-first enrollment handshake on the
//! public server: begin → sign → complete, plus the opaque-error and
//! token-state behaviours.

use base64::Engine;
use commons_tests::server::{make_signing_certificate, run};
use commons_types::server::{TagMap, kind::ServerKind};
use database::{
	pg_duration::PgDuration, server_enrollment_tokens::ServerEnrollmentToken, servers::Server,
	url_field::UrlField,
};
use jiff::SignedDuration;
use serde_json::{Value, json};
use uuid::Uuid;

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
	base64::engine::general_purpose::STANDARD
}

fn new_server(host: &str) -> Server {
	Server {
		id: Uuid::new_v4(),
		name: Some("test".into()),
		host: UrlField(host.parse().unwrap()),
		kind: ServerKind::Central,
		rank: None,
		device_id: None,
		group_id: None,
		public_name: None,
		cloud: None,
		geolocation: None,
		is_monitored: true,
		alert_when_down_for: PgDuration(SignedDuration::from_secs(600)),
		notes: String::new(),
		tags: TagMap::default(),
		deleted_at: None,
		registered_at: None,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn enrollment_happy_path() {
	run(async |mut conn, public, _private| {
		let (spki, cert, signing_key) = make_signing_certificate();
		let server = Server::create(&mut conn, new_server("https://happy.example/"))
			.await
			.unwrap();
		let (_t, token) =
			ServerEnrollmentToken::mint(&mut conn, server.id, SignedDuration::from_hours(1))
				.await
				.unwrap();

		// begin
		let resp = public
			.post("/servers/register/begin")
			.add_header("mtls-certificate", &cert)
			.json(&json!({"server_id": server.id, "token": token}))
			.await;
		resp.assert_status_ok();
		let begin: Value = resp.json();
		let nonce_b64 = begin["nonce"].as_str().unwrap().to_string();
		assert_eq!(begin["channel_binding_required"], json!(false));
		let nonce = b64().decode(&nonce_b64).unwrap();

		// sign transcript: nonce ‖ server_id ‖ spki
		let mut transcript = nonce.clone();
		transcript.extend_from_slice(server.id.as_bytes());
		transcript.extend_from_slice(&spki);
		let rng = ring::rand::SystemRandom::new();
		let sig = signing_key.sign(&rng, &transcript).unwrap();
		let sig_b64 = b64().encode(sig.as_ref());

		// complete
		let resp = public
			.post("/servers/register/complete")
			.add_header("mtls-certificate", &cert)
			.json(&json!({"server_id": server.id, "nonce": nonce_b64, "signature": sig_b64}))
			.await;
		resp.assert_status_ok();

		// server is now registered + bound; token is spent.
		let after = Server::get_by_id(&mut conn, server.id).await.unwrap();
		assert!(after.registered_at.is_some(), "registered_at set");
		assert!(after.device_id.is_some(), "device bound");
		assert!(
			ServerEnrollmentToken::find_active(&mut conn, server.id, &token)
				.await
				.is_err(),
			"token consumed"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn enrollment_bad_signature_is_opaque_and_keeps_token() {
	run(async |mut conn, public, _private| {
		let (_spki, cert, _key) = make_signing_certificate();
		let server = Server::create(&mut conn, new_server("https://badsig.example/"))
			.await
			.unwrap();
		let (_t, token) =
			ServerEnrollmentToken::mint(&mut conn, server.id, SignedDuration::from_hours(1))
				.await
				.unwrap();

		let resp = public
			.post("/servers/register/begin")
			.add_header("mtls-certificate", &cert)
			.json(&json!({"server_id": server.id, "token": token}))
			.await;
		resp.assert_status_ok();
		let nonce_b64 = resp.json::<Value>()["nonce"].as_str().unwrap().to_string();

		// garbage signature
		let resp = public
			.post("/servers/register/complete")
			.add_header("mtls-certificate", &cert)
			.json(&json!({
				"server_id": server.id,
				"nonce": nonce_b64,
				"signature": b64().encode([0u8; 72]),
			}))
			.await;
		resp.assert_status_forbidden();

		// token must NOT have been burned by a failed complete.
		assert!(
			ServerEnrollmentToken::find_active(&mut conn, server.id, &token)
				.await
				.is_ok(),
			"token still active after bad signature"
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn enrollment_unknown_server_and_bad_token_are_opaque() {
	run(async |mut conn, public, _private| {
		let (_spki, cert, _key) = make_signing_certificate();

		// unknown server id
		let resp = public
			.post("/servers/register/begin")
			.add_header("mtls-certificate", &cert)
			.json(&json!({"server_id": Uuid::new_v4(), "token": "whatever"}))
			.await;
		resp.assert_status_forbidden();

		// known server, wrong token
		let server = Server::create(&mut conn, new_server("https://badtok.example/"))
			.await
			.unwrap();
		let resp = public
			.post("/servers/register/begin")
			.add_header("mtls-certificate", &cert)
			.json(&json!({"server_id": server.id, "token": "not-the-token"}))
			.await;
		resp.assert_status_forbidden();
	})
	.await;
}
