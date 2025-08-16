#[path = "./db.rs"]
mod test_db;
use axum::extract::connect_info::MockConnectInfo;
use axum_test::TestServer;
use diesel_async::AsyncPgConnection;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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

		// Add ConnectInfo layer for test servers
		let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
		let public_router =
			router(state.clone(), public_routes()).layer(MockConnectInfo(mock_addr));
		let private_router =
			router(state.clone(), private_routes("/$".into())).layer(MockConnectInfo(mock_addr));

		test(
			conn,
			TestServer::new(public_router).unwrap(),
			TestServer::new(private_router).unwrap(),
		)
		.await
	})
	.await
}
