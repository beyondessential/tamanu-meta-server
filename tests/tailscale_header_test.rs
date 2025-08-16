use axum_test::TestServer;
use tamanu_meta::{private_routes, state::AppState};

#[tokio::test]
async fn test_tailscale_header_extraction() {
	// Create a test server with the private routes
	let state = AppState::init().expect("Failed to initialize app state");
	let app = private_routes("/$".to_string()).with_state(state);
	let server = TestServer::new(app).expect("Failed to create test server");

	// Test without Tailscale-User-Name header (should show "Kia Ora!")
	let response = server.get("/$/status").await;
	response.assert_status_ok();
	let body = response.text();
	assert!(body.contains("Kia Ora!"));
	assert!(!body.contains("Hi "));

	// Test with Tailscale-User-Name header (should show "Hi John!")
	let response = server
		.get("/$/status")
		.add_header("Tailscale-User-Name", "John")
		.await;
	response.assert_status_ok();
	let body = response.text();
	assert!(body.contains("Hi John!"));
	assert!(!body.contains("Kia Ora!"));

	// Test with different user name
	let response = server
		.get("/$/status")
		.add_header("Tailscale-User-Name", "Alice")
		.await;
	response.assert_status_ok();
	let body = response.text();
	assert!(body.contains("Hi Alice!"));
	assert!(!body.contains("Kia Ora!"));
}
