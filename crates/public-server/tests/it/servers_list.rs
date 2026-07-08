use diesel_async::SimpleAsyncConnection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PublicServer {
	pub name: String,
	pub host: String,
	pub rank: Option<String>,
}

// GET /servers tests. (The device-driven create/edit/remove endpoints were
// removed in favour of operator-first enrollment; see server_enrollment.rs.)

#[tokio::test(flavor = "multi_thread")]
async fn get_empty_list() {
	commons_tests::server::run(async |_conn, public, _| {
		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<PublicServer>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_with_central_server() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO servers (name, host, kind, rank, public_name) VALUES ('Test Server', 'https://test.com', 'central', 'production', 'Test Server')",
		)
		.await
		.unwrap();

		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<PublicServer>>(&vec![PublicServer {
			name: "Test Server".to_string(),
			host: "https://test.com".to_string(),
			rank: Some("production".to_string()),
		}]);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_without_public_name() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('Internal Server', 'https://test.com', 'central', 'production')",
		)
		.await
		.unwrap();

		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<PublicServer>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_filters_facility_servers() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO servers (name, host, kind, rank, public_name) VALUES
			('Central Server', 'https://central.com', 'central', 'production', 'Central Server'),
			('Facility Server', 'https://facility.com', 'facility', 'production', NULL)",
		)
		.await
		.unwrap();

		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<PublicServer>>(&vec![PublicServer {
			name: "Central Server".to_string(),
			host: "https://central.com".to_string(),
			rank: Some("production".to_string()),
		}]);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_multiple_central_servers() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO servers (name, host, kind, rank, public_name) VALUES
			('Server A', 'https://a.com', 'central', 'production', 'Server A'),
			('Server B', 'https://b.com', 'central', 'staging', 'Server B')",
		)
		.await
		.unwrap();

		let response = public.get("/servers").await;
		response.assert_status_ok();
		let servers: Vec<PublicServer> = response.json();
		assert_eq!(servers.len(), 2);
		assert!(
			servers
				.iter()
				.any(|s| s.name == "Server A" && s.host == "https://a.com")
		);
		assert!(
			servers
				.iter()
				.any(|s| s.name == "Server B" && s.host == "https://b.com")
		);
	})
	.await
}
