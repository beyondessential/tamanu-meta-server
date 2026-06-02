//! Round-trips a minted enrollment ticket: the private API returns an
//! age/scrypt-encrypted ticket plus the 4-word passphrase; decrypting the
//! ticket with the passphrase must yield the real, active enrollment token.

use age::secrecy::SecretString;
use algae_cli::{passphrases::Passphrase, streams::decrypt_stream};
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
