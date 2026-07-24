use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
struct SelfResponse {
	server_id: Uuid,
	device_id: Uuid,
}

/// A registered, attached device recovers its own identity: the server it is
/// enrolled as and its own device ID.
#[tokio::test(flavor = "multi_thread")]
async fn self_endpoint_returns_server_and_device_ids() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				"INSERT INTO servers (id, host, kind, device_id) \
				 VALUES ($1, 'https://self.example.com', 'central', $2)",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Uuid, _>(device_id)
			.execute(&mut conn)
			.await
			.unwrap();

			let response = public
				.get("/servers/self")
				.add_header("mtls-certificate", &cert)
				.await;
			response.assert_status_ok();
			let body: SelfResponse = response.json();
			assert_eq!(body.server_id, server_id);
			assert_eq!(body.device_id, device_id);
		},
	)
	.await
}

/// A device that authenticates correctly but isn't attached to any server
/// gets a 412 (precondition failed), matching the `/tags` endpoint.
#[tokio::test(flavor = "multi_thread")]
async fn self_endpoint_412_when_device_has_no_server() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut _conn, cert, _device_id, public, _| {
			let response = public
				.get("/servers/self")
				.add_header("mtls-certificate", &cert)
				.await;
			response.assert_status(StatusCode::PRECONDITION_FAILED);
		},
	)
	.await
}

/// A request with no client certificate is unauthenticated.
#[tokio::test(flavor = "multi_thread")]
async fn self_endpoint_401_without_certificate() {
	commons_tests::server::run(async |_conn, public, _private| {
		let response = public.get("/servers/self").await;
		response.assert_status(StatusCode::UNAUTHORIZED);
	})
	.await
}
