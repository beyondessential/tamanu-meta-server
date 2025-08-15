#[path = "./db.rs"]
mod test_db;
use axum_test::TestServer;
use diesel_async::AsyncPgConnection;
use tamanu_meta::{private_routes, public_routes, router, state::AppState};
use test_db::TestDb;

pub async fn run<F, T, Fut>(test: F) -> T
where
	F: FnOnce(AsyncPgConnection, TestServer, TestServer) -> Fut,
	Fut: Future<Output = T>,
{
	TestDb::run(async |conn, url| {
		let state = AppState {
			db: AppState::init_db_to(&url),
			tera: AppState::init_tera().unwrap(),
		};

		test(
			conn,
			TestServer::new(router(state.clone(), public_routes())).unwrap(),
			TestServer::new(router(state.clone(), private_routes("/$".into()))).unwrap(),
		)
		.await
	})
	.await
}
