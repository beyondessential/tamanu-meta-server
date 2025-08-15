use diesel_async::SimpleAsyncConnection;
use serde::Deserialize;

#[path = "common/server.rs"]
mod test_server;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Server {
	pub name: String,
	pub host: String,
	pub rank: Option<String>,
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_list() {
	test_server::run(async |_conn, public, _| {
		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<Server>>(&Vec::new());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn with_server() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO servers (name, host, rank) VALUES ('Test Server', 'https://test.com', 'production')",
		)
		.await
		.unwrap();

		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<Server>>(&vec![
			Server {
				name: "Test Server".to_string(),
				host: "https://test.com".to_string(),
				rank: Some("production".to_string()),
			}
		]);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn with_unnamed_server() {
	test_server::run(async |mut conn, public, _| {
		conn.batch_execute(
			"INSERT INTO servers (host, rank) VALUES ('https://test.com', 'production')",
		)
		.await
		.unwrap();

		let response = public.get("/servers").await;
		response.assert_status_ok();
		response.assert_json::<Vec<Server>>(&Vec::new());
	})
	.await
}
