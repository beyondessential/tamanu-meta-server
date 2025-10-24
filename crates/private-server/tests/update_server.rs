use axum::http::StatusCode;
use commons_tests::diesel_async::SimpleAsyncConnection;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ServerDetailsResponse {
	id: String,
	name: String,
	kind: String,
	rank: String,
	host: String,
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_basic_fields() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('22222222-2222-2222-2222-222222222222', 'Original Server', 'https://original.example.com', 'test', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/statuses/update_server")
			.form(&[
				("server_id", "22222222-2222-2222-2222-222222222222"),
				("name", "Updated Server"),
				("host", "https://updated.example.com"),
				("rank", "production"),
			])
			.await;
		response.assert_status_ok();
		let updated: ServerDetailsResponse = response.json();

		assert_eq!(updated.name, "Updated Server");
		assert_eq!(updated.host, "https://updated.example.com/");
		assert_eq!(updated.rank, "production");
		assert_eq!(updated.kind, "central");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_partial_update() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('33333333-3333-3333-3333-333333333333', 'Partial Server', 'https://partial.example.com', 'demo', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/statuses/update_server")
			.form(&[
				("server_id", "33333333-3333-3333-3333-333333333333"),
				("rank", "clone"),
			])
			.await;
		response.assert_status_ok();
		let updated: ServerDetailsResponse = response.json();

		assert_eq!(updated.name, "Partial Server");
		assert_eq!(updated.host, "https://partial.example.com/");
		assert_eq!(updated.rank, "clone");
		assert_eq!(updated.kind, "central");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_device_id() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES
			('44444444-4444-4444-4444-444444444444', 'server')"
		)
		.await
		.unwrap();

		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('55555555-5555-5555-5555-555555555555', 'Device Server', 'https://device.example.com', 'production', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/statuses/update_server")
			.form(&[
				("server_id", "55555555-5555-5555-5555-555555555555"),
				("device_id", "44444444-4444-4444-4444-444444444444"),
			])
			.await;
		response.assert_status_ok();
		let updated: ServerDetailsResponse = response.json();

		assert_eq!(updated.id, "55555555-5555-5555-5555-555555555555");
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_invalid_rank() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute(
			"INSERT INTO servers (id, name, host, rank, kind) VALUES
			('66666666-6666-6666-6666-666666666666', 'Rank Server', 'https://rank.example.com', 'test', 'central')"
		)
		.await
		.unwrap();

		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/statuses/update_server")
			.form(&[
				("server_id", "66666666-6666-6666-6666-666666666666"),
				("rank", "invalid_rank"),
			])
			.await;
		response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_server_not_found() {
	commons_tests::server::run(async |mut conn, _, private| {
		conn.batch_execute("INSERT INTO admins (email) VALUES ('admin@example.com')")
			.await
			.unwrap();

		let response = private
			.post("/api/private_server/fns/statuses/update_server")
			.form(&[
				("server_id", "77777777-7777-7777-7777-777777777777"),
				("name", "Non-existent"),
			])
			.await;
		response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
	})
	.await
}

// Note: Admin authentication test cannot be properly tested in debug mode
// because TailscaleAdmin always returns admin@localhost in debug_assertions mode
