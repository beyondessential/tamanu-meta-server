//! `Status::ping_server` decides whether a server counts as reachable.
//!
//! `reqwest`'s `send()` resolves to `Ok` for any HTTP response, so a naive
//! match treats a 502 from a reverse proxy — the app behind it being down,
//! which is precisely the outage the reachability sweep exists to catch — as
//! a successful ping.

use axum::http::StatusCode;
use commons_tests::db::TestDb;
use database::servers::Server;
use database::statuses::Status;
use diesel_async::SimpleAsyncConnection;
use uuid::Uuid;

/// Spawn a one-route server on a random port that answers `/api/public/ping`
/// with `status`, and return its base URL.
async fn ping_endpoint_returning(status: StatusCode, version: Option<&'static str>) -> String {
	let app = axum::Router::new().route(
		"/api/public/ping",
		axum::routing::get(move || async move {
			let mut resp = axum::response::Response::builder().status(status);
			if let Some(v) = version {
				resp = resp.header("X-Version", v);
			}
			resp.body(axum::body::Body::empty()).unwrap()
		}),
	);
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	tokio::spawn(async move {
		axum::serve(listener, app.into_make_service()).await.ok();
	});
	format!("http://{addr}/")
}

async fn seed_server(conn: &mut database::diesel_async::AsyncPgConnection, host: &str) -> Server {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO servers (id, host, name, kind) VALUES ('{id}', '{host}', 'pinged', 'central');"
	))
	.await
	.expect("seed server");
	Server::get_by_id(conn, id).await.expect("fetch server")
}

async fn ping(server: &Server) -> Option<Status> {
	let client = reqwest::ClientBuilder::new()
		.timeout(std::time::Duration::from_secs(5))
		.build()
		.unwrap();
	Status::ping_server(&client, server).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_5xx_from_the_ping_endpoint_is_not_reachable() {
	TestDb::run(|mut conn, _url| async move {
		let host = ping_endpoint_returning(StatusCode::BAD_GATEWAY, None).await;
		let server = seed_server(&mut conn, &host).await;
		assert!(
			ping(&server).await.is_none(),
			"a 502 means the app behind the proxy is down, not that the server is up",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_404_from_the_ping_endpoint_is_not_reachable() {
	TestDb::run(|mut conn, _url| async move {
		let host = ping_endpoint_returning(StatusCode::NOT_FOUND, None).await;
		let server = seed_server(&mut conn, &host).await;
		assert!(
			ping(&server).await.is_none(),
			"nothing answered the ping endpoint, so nothing is confirmed reachable",
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_200_is_reachable_and_carries_the_version() {
	TestDb::run(|mut conn, _url| async move {
		let host = ping_endpoint_returning(StatusCode::OK, Some("2.31.0")).await;
		let server = seed_server(&mut conn, &host).await;
		let status = ping(&server).await.expect("a 200 is a reachable server");
		assert!(status.healthy);
		assert_eq!(
			status.version.map(|v| v.to_string()),
			Some("2.31.0".to_string()),
		);
	})
	.await;
}

/// A 200 without the header is still reachable — the version is simply
/// unknown. This is the case the old code conflated every failure with.
#[tokio::test(flavor = "multi_thread")]
async fn a_200_without_a_version_header_is_still_reachable() {
	TestDb::run(|mut conn, _url| async move {
		let host = ping_endpoint_returning(StatusCode::OK, None).await;
		let server = seed_server(&mut conn, &host).await;
		let status = ping(&server).await.expect("reachable");
		assert!(status.version.is_none());
	})
	.await;
}
