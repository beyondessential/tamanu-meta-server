use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::{RunQueryDsl, SimpleAsyncConnection};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct PublicServer {
	pub name: String,
	pub host: String,
	pub rank: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewServer {
	pub name: Option<String>,
	pub host: String,
	pub kind: String,
	pub rank: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PartialServer {
	pub id: Uuid,
	pub name: Option<String>,
	pub host: Option<String>,
	pub kind: Option<String>,
	pub rank: Option<String>,
}

#[derive(QueryableByName)]
struct ServerRow {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	name: Option<String>,
	#[diesel(sql_type = sql_types::Text)]
	host: String,
	#[diesel(sql_type = sql_types::Text)]
	kind: String,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	rank: Option<String>,
}

#[derive(QueryableByName)]
struct IdRow {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

// GET /servers tests
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
			"INSERT INTO servers (name, host, kind, rank) VALUES ('Test Server', 'https://test.com', 'central', 'production')",
		)
		.await
		.unwrap();

		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<PublicServer>>(&vec![
			PublicServer {
				name: "Test Server".to_string(),
				host: "https://test.com".to_string(),
				rank: Some("production".to_string()),
			}
		]);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn get_with_unnamed_server() {
	commons_tests::server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO servers (host, kind, rank) VALUES ('https://test.com', 'central', 'production')",
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
			"INSERT INTO servers (name, host, kind, rank) VALUES
			('Central Server', 'https://central.com', 'central', 'production'),
			('Facility Server', 'https://facility.com', 'facility', 'production')",
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
			"INSERT INTO servers (name, host, kind, rank) VALUES
			('Server A', 'https://a.com', 'central', 'production'),
			('Server B', 'https://b.com', 'central', 'staging')",
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

// POST /servers tests
#[tokio::test(flavor = "multi_thread")]
async fn post_create_server_success() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, _device_id, public, _| {
			let new_server = NewServer {
				name: Some("New Server".to_string()),
				host: "https://newserver.com".to_string(),
				kind: "central".to_string(),
				rank: Some("dev".to_string()),
			};

			let response = public
				.post("/servers")
				.add_header("mtls-certificate", &cert)
				.json(&new_server)
				.await;
			response.assert_status_ok();

			// Verify server was created in database
			let servers: Vec<ServerRow> = sql_query(
				"SELECT id, name, host, kind, rank FROM servers WHERE name = 'New Server'",
			)
			.get_results(&mut conn)
			.await
			.unwrap();

			assert_eq!(servers.len(), 1);
			assert_eq!(servers[0].name, Some("New Server".to_string()));
			assert_eq!(servers[0].host, "https://newserver.com/");
			assert_eq!(servers[0].kind, "central");
			assert_eq!(servers[0].rank, Some("dev".to_string()));
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn post_create_server_unauthorized() {
	commons_tests::server::run(async |_conn, public, _| {
		let new_server = NewServer {
			name: Some("New Server".to_string()),
			host: "https://newserver.com".to_string(),
			kind: "central".to_string(),
			rank: Some("development".to_string()),
		};

		let response = public.post("/servers").json(&new_server).await;
		response.assert_status(StatusCode::UNAUTHORIZED); // Should fail without auth
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn post_create_server_with_admin_device() {
	commons_tests::server::run_with_device_auth(
		"admin",
		async |mut conn, cert, _device_id, public, _| {
			let new_server = NewServer {
				name: Some("Admin Server".to_string()),
				host: "https://adminserver.com".to_string(),
				kind: "central".to_string(),
				rank: Some("production".to_string()),
			};

			let response = public
				.post("/servers")
				.add_header("mtls-certificate", &cert)
				.json(&new_server)
				.await;
			response.assert_status_ok();

			// Verify server was created
			let servers: Vec<ServerRow> = sql_query(
				"SELECT id, name, host, kind, rank FROM servers WHERE name = 'Admin Server'",
			)
			.get_results(&mut conn)
			.await
			.unwrap();

			assert_eq!(servers.len(), 1);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn post_create_server_invalid_role() {
	commons_tests::server::run_with_device_auth(
		"releaser",
		async |_conn, cert, _device_id, public, _| {
			let new_server = NewServer {
				name: Some("Releaser Server".to_string()),
				host: "https://releaserserver.com".to_string(),
				kind: "central".to_string(),
				rank: Some("production".to_string()),
			};

			let response = public
				.post("/servers")
				.add_header("mtls-certificate", &cert)
				.json(&new_server)
				.await;
			response.assert_status(StatusCode::FORBIDDEN); // Releaser role should not be able to create servers
		},
	)
	.await
}

// PATCH /servers tests
#[tokio::test(flavor = "multi_thread")]
async fn patch_edit_server_success() {
	commons_tests::server::run_with_device_auth("server", async |mut conn, cert, _device_id, public, _| {
		// Create a server first
		let server_row: IdRow = sql_query(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('Original Server', 'https://original.com', 'central', 'dev') RETURNING id"
		)
		.get_result(&mut conn)
		.await
		.unwrap();
		let server_id = server_row.id;

		let partial_server = PartialServer {
			id: server_id,
			name: Some("Updated Server".to_string()),
			host: Some("https://updated.com".to_string()),
			kind: None,
			rank: Some("production".to_string()),
		};

		let response = public.patch("/servers")
			.add_header("mtls-certificate", &cert)
			.json(&partial_server)
			.await;
		response.assert_status_ok();

		// Verify server was updated
		let servers: Vec<ServerRow> = sql_query("SELECT id, name, host, kind, rank FROM servers WHERE id = $1")
			.bind::<sql_types::Uuid, _>(server_id)
			.get_results(&mut conn)
			.await
			.unwrap();

		assert_eq!(servers.len(), 1);
		assert_eq!(servers[0].name, Some("Updated Server".to_string()));
		assert_eq!(servers[0].host, "https://updated.com/");
		assert_eq!(servers[0].rank, Some("production".to_string()));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn patch_edit_server_unauthorized() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create a server first
		let server_row: IdRow = sql_query(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('Original Server', 'https://original.com', 'central', 'dev') RETURNING id"
		)
		.get_result(&mut conn)
		.await
		.unwrap();
		let server_id = server_row.id;

		let partial_server = PartialServer {
			id: server_id,
			name: Some("Updated Server".to_string()),
			host: None,
			kind: None,
			rank: None,
		};

		let response = public.patch("/servers").json(&partial_server).await;
		response.assert_status(StatusCode::UNAUTHORIZED); // Should fail without auth
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn patch_edit_nonexistent_server() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |_conn, cert, _device_id, public, _| {
			let nonexistent_id = Uuid::new_v4();
			let partial_server = PartialServer {
				id: nonexistent_id,
				name: Some("Updated Server".to_string()),
				host: None,
				kind: None,
				rank: None,
			};

			let response = public
				.patch("/servers")
				.add_header("mtls-certificate", &cert)
				.json(&partial_server)
				.await;
			response.assert_status_not_ok(); // Should fail for nonexistent server (could be 404 or 500)
		},
	)
	.await
}

// DELETE /servers tests
#[tokio::test(flavor = "multi_thread")]
async fn delete_server_success_with_admin() {
	commons_tests::server::run_with_device_auth("admin", async |mut conn, cert, _device_id, public, _| {
		// Create a server first
		let server_row: IdRow = sql_query(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('To Delete', 'https://todelete.com', 'central', 'dev') RETURNING id"
		)
		.get_result(&mut conn)
		.await
		.unwrap();
		let server_id = server_row.id;

		let partial_server = PartialServer {
			id: server_id,
			name: None,
			host: None,
			kind: None,
			rank: None,
		};

		let response = public.delete("/servers")
			.add_header("mtls-certificate", &cert)
			.json(&partial_server)
			.await;
		response.assert_status_ok();

		// Verify server was deleted
		let servers: Vec<ServerRow> = sql_query("SELECT id, name, host, kind, rank FROM servers WHERE id = $1")
			.bind::<sql_types::Uuid, _>(server_id)
			.get_results(&mut conn)
			.await
			.unwrap();

		assert_eq!(servers.len(), 0);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_server_unauthorized_no_auth() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create a server first
		let server_row: IdRow = sql_query(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('To Delete', 'https://todelete.com', 'central', 'dev') RETURNING id"
		)
		.get_result(&mut conn)
		.await
		.unwrap();
		let server_id = server_row.id;

		let partial_server = PartialServer {
			id: server_id,
			name: None,
			host: None,
			kind: None,
			rank: None,
		};

		let response = public.delete("/servers").json(&partial_server).await;
		response.assert_status(StatusCode::UNAUTHORIZED); // Should fail without auth
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_server_unauthorized_server_role() {
	commons_tests::server::run_with_device_auth("server", async |mut conn, cert, _device_id, public, _| {
		// Create a server first
		let server_row: IdRow = sql_query(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('To Delete', 'https://todelete.com', 'central', 'dev') RETURNING id"
		)
		.get_result(&mut conn)
		.await
		.unwrap();
		let server_id = server_row.id;

		let partial_server = PartialServer {
			id: server_id,
			name: None,
			host: None,
			kind: None,
			rank: None,
		};

		let response = public.delete("/servers")
			.add_header("mtls-certificate", &cert)
			.json(&partial_server)
			.await;
		response.assert_status(StatusCode::FORBIDDEN); // Server role should not be able to delete, only admin
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_nonexistent_server() {
	commons_tests::server::run_with_device_auth(
		"admin",
		async |_conn, cert, _device_id, mut public, _| {
			let nonexistent_id = Uuid::new_v4();
			let partial_server = PartialServer {
				id: nonexistent_id,
				name: None,
				host: None,
				kind: None,
				rank: None,
			};

			public.add_header("mtls-certificate", &cert);
			let response = public.delete("/servers").json(&partial_server).await;
			response.assert_status_ok(); // DELETE should succeed even for nonexistent server
		},
	)
	.await
}

// Authentication-specific error tests
#[tokio::test(flavor = "multi_thread")]
async fn post_create_server_invalid_certificate() {
	commons_tests::server::run(async |_conn, public, _| {
		let new_server = NewServer {
			name: Some("Test Server".to_string()),
			host: "https://test.example.com".to_string(),
			kind: "central".to_string(),
			rank: Some("dev".to_string()),
		};

		// Send invalid certificate data
		let response = public
			.post("/servers")
			.add_header("mtls-certificate", "invalid-certificate-data")
			.json(&new_server)
			.await;
		response.assert_status(StatusCode::BAD_REQUEST); // Invalid certificate format
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn post_create_server_malformed_pem() {
	commons_tests::server::run(async |_conn, public, _| {
		let new_server = NewServer {
			name: Some("Test Server".to_string()),
			host: "https://test.example.com".to_string(),
			kind: "central".to_string(),
			rank: Some("dev".to_string()),
		};

		// Send malformed PEM certificate
		use percent_encoding::utf8_percent_encode;
		let malformed_pem = utf8_percent_encode(
			"-----BEGIN CERTIFICATE-----\ninvalid-data\n-----END CERTIFICATE-----",
			&percent_encoding::NON_ALPHANUMERIC,
		)
		.to_string();

		let response = public
			.post("/servers")
			.add_header("mtls-certificate", &malformed_pem)
			.json(&new_server)
			.await;
		response.assert_status(StatusCode::BAD_REQUEST); // Invalid certificate format
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn post_create_server_empty_certificate() {
	commons_tests::server::run(async |_conn, public, _| {
		let new_server = NewServer {
			name: Some("Test Server".to_string()),
			host: "https://test.example.com".to_string(),
			kind: "central".to_string(),
			rank: Some("dev".to_string()),
		};

		// Send empty certificate header
		let response = public
			.post("/servers")
			.add_header("mtls-certificate", "")
			.json(&new_server)
			.await;
		response.assert_status(StatusCode::BAD_REQUEST); // Invalid certificate format
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn patch_edit_server_invalid_certificate() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create a server first
		let server_row: IdRow = sql_query(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('Test Server', 'https://test.com', 'central', 'dev') RETURNING id"
		)
		.get_result(&mut conn)
		.await
		.unwrap();

		let partial_server = PartialServer {
			id: server_row.id,
			name: Some("Updated Server".to_string()),
			host: None,
			kind: None,
			rank: None,
		};

		// Send invalid certificate data
		let response = public.patch("/servers")
			.add_header("mtls-certificate", "invalid-cert")
			.json(&partial_server)
			.await;
		response.assert_status(StatusCode::BAD_REQUEST); // Invalid certificate format
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_server_invalid_certificate() {
	commons_tests::server::run(async |mut conn, public, _| {
		// Create a server first
		let server_row: IdRow = sql_query(
			"INSERT INTO servers (name, host, kind, rank) VALUES ('Test Server', 'https://test.com', 'central', 'dev') RETURNING id"
		)
		.get_result(&mut conn)
		.await
		.unwrap();

		let partial_server = PartialServer {
			id: server_row.id,
			name: None,
			host: None,
			kind: None,
			rank: None,
		};

		// Send invalid certificate data
		let response = public.delete("/servers")
			.add_header("mtls-certificate", "invalid-cert")
			.json(&partial_server)
			.await;
		response.assert_status(StatusCode::BAD_REQUEST); // Invalid certificate format
	})
	.await
}

// Integration tests with mixed scenarios
#[tokio::test(flavor = "multi_thread")]
async fn integration_full_crud_cycle() {
	commons_tests::server::run_with_device_auth(
		"admin",
		async |mut conn, cert, _device_id, public, _| {
			// Create a server
			let new_server = NewServer {
				name: Some("CRUD Test Server".to_string()),
				host: "https://crudtest.com".to_string(),
				kind: "central".to_string(),
				rank: Some("clone".to_string()),
			};

			let response = public
				.post("/servers")
				.add_header("mtls-certificate", &cert)
				.json(&new_server)
				.await;
			response.assert_status_ok();

			// Get the created server from the list
			let response = public.get("/servers").await;
			response.assert_status_ok();
			let servers: Vec<PublicServer> = response.json();
			let _created_server = servers
				.iter()
				.find(|s| s.name == "CRUD Test Server")
				.unwrap();

			// Get server ID from database for editing/deletion
			let server_rows: Vec<ServerRow> = sql_query(
				"SELECT id, name, host, kind, rank FROM servers WHERE name = 'CRUD Test Server'",
			)
			.get_results(&mut conn)
			.await
			.unwrap();
			let server_id = server_rows[0].id;

			// Update the server
			let partial_server = PartialServer {
				id: server_id,
				name: Some("Updated CRUD Server".to_string()),
				host: None,
				kind: None,
				rank: Some("production".to_string()),
			};

			let response = public
				.patch("/servers")
				.add_header("mtls-certificate", &cert)
				.json(&partial_server)
				.await;
			response.assert_status_ok();

			// Verify update in list
			let response = public.get("/servers").await;
			response.assert_status_ok();
			let servers: Vec<PublicServer> = response.json();
			let updated_server = servers
				.iter()
				.find(|s| s.name == "Updated CRUD Server")
				.unwrap();
			assert_eq!(updated_server.rank, Some("production".to_string()));

			// Delete the server
			let delete_request = PartialServer {
				id: server_id,
				name: None,
				host: None,
				kind: None,
				rank: None,
			};

			let response = public
				.delete("/servers")
				.add_header("mtls-certificate", &cert)
				.json(&delete_request)
				.await;
			response.assert_status_ok();

			// Verify deletion
			let response = public.get("/servers").await;
			response.assert_status_ok();
			let servers: Vec<PublicServer> = response.json();
			assert!(!servers.iter().any(|s| s.name == "Updated CRUD Server"));
		},
	)
	.await
}
