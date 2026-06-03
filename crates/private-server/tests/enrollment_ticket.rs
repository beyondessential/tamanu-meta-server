//! Round-trips a minted enrollment ticket: the private API returns an
//! age/scrypt-encrypted ticket plus the 4-word passphrase; decrypting the
//! ticket with the passphrase must yield the real, active enrollment token.

use algae_cli::{
	passphrases::{Passphrase, SecretString},
	streams::decrypt_stream,
};
use base64::Engine;
use database::server_enrollment_tokens::ServerEnrollmentToken;
use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread")]
async fn mint_enrollment_ticket_round_trips_to_active_token() {
	commons_tests::server::run(async |mut conn, _public, private| {
		// PUBLIC_URL is read by the handler to build the payload.
		unsafe { std::env::set_var("PUBLIC_URL", "https://api.example.test") };

		// Create a server to enrol.
		let response = private
			.post("/api/servers/create")
			.json(&json!({ "kind": "central" }))
			.await;
		response.assert_status_ok();
		let server_id: String = response.json();

		// Mint an encrypted enrollment ticket.
		let response = private
			.post("/api/servers/mint_enrollment")
			.json(&json!({ "server_id": server_id }))
			.await;
		response.assert_status_ok();
		let body: Value = response.json();
		let ticket = body["ticket"].as_str().expect("ticket present");
		let passphrase = body["passphrase"].as_str().expect("passphrase present");

		// The passphrase is 4 lowercase hyphen-separated words.
		let words: Vec<&str> = passphrase.split('-').collect();
		assert_eq!(words.len(), 4, "passphrase is four words: {passphrase}");
		assert!(
			words.iter().all(|w| !w.is_empty() && w == &w.to_lowercase()),
			"passphrase words are non-empty and lowercase: {passphrase}"
		);

		// Decrypt the ticket with the passphrase.
		let encrypted = base64::engine::general_purpose::STANDARD
			.decode(ticket)
			.expect("ticket is valid base64");
		let key = Passphrase::new(SecretString::from(passphrase.to_string()));
		let mut decrypted = Vec::new();
		decrypt_stream(
			futures::io::Cursor::new(&encrypted[..]),
			&mut decrypted,
			Box::new(key),
		)
		.await
		.expect("ticket decrypts with the returned passphrase");

		// (a) The decrypted payload carries a token.
		let payload: Value = serde_json::from_slice(&decrypted).expect("payload is JSON");
		assert_eq!(payload["v"], "enroll-1");
		assert_eq!(payload["server_id"], server_id);
		let token = payload["token"].as_str().expect("token in payload");

		// (b) That token is the real, active enrollment token for the server.
		ServerEnrollmentToken::find_active(&mut conn, server_id.parse().unwrap(), token)
			.await
			.expect("decrypted token is active for the server");
	})
	.await
}

/// The plaintext enrollment token is delivered *only* inside the encrypted
/// ticket — it must never appear in the clear in any response or error body.
#[tokio::test(flavor = "multi_thread")]
async fn enrollment_token_never_leaks_in_the_clear() {
	commons_tests::server::run(async |_conn, public, private| {
		unsafe { std::env::set_var("PUBLIC_URL", "https://api.example.test") };

		let response = private
			.post("/api/servers/create")
			.json(&json!({ "kind": "central" }))
			.await;
		response.assert_status_ok();
		let server_id: String = response.json();

		let mint = private
			.post("/api/servers/mint_enrollment")
			.json(&json!({ "server_id": server_id }))
			.await;
		mint.assert_status_ok();
		let mint_body = mint.text();
		let body: Value = serde_json::from_str(&mint_body).expect("mint body is JSON");
		let ticket = body["ticket"].as_str().expect("ticket present");
		let passphrase = body["passphrase"].as_str().expect("passphrase present");

		// Recover the plaintext token by decrypting the ticket — the only place
		// it should be obtainable.
		let encrypted = base64::engine::general_purpose::STANDARD
			.decode(ticket)
			.expect("ticket is valid base64");
		let key = Passphrase::new(SecretString::from(passphrase.to_string()));
		let mut decrypted = Vec::new();
		decrypt_stream(
			futures::io::Cursor::new(&encrypted[..]),
			&mut decrypted,
			Box::new(key),
		)
		.await
		.expect("ticket decrypts");
		let payload: Value = serde_json::from_slice(&decrypted).expect("payload is JSON");
		let token = payload["token"].as_str().expect("token in payload");
		assert!(!token.is_empty(), "token is non-empty");

		// (a) The mint response carries the token only as ciphertext, never in
		// the clear.
		assert!(
			!mint_body.contains(token),
			"mint response must not contain the plaintext token"
		);

		// (b) enrollment_status returns only timestamps — never the token.
		let status = private
			.post("/api/servers/enrollment_status")
			.json(&json!({ "server_id": server_id }))
			.await;
		status.assert_status_ok();
		assert!(
			!status.text().contains(token),
			"enrollment_status must not contain the token"
		);

		// (c) A failed register (no client cert) returns an opaque error that
		// does not echo the token back, even though we sent it.
		let begin = public
			.post("/servers/register/begin")
			.json(&json!({ "server_id": server_id, "token": token }))
			.await;
		begin.assert_status_forbidden();
		assert!(
			!begin.text().contains(token),
			"a register error must not echo the token"
		);
	})
	.await
}
