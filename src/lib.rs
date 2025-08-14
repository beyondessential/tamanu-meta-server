use std::{net::SocketAddr, time::Duration};

use axum::Router;
use state::AppState;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::Span;

pub use servers::private::routes as private_routes;
pub use servers::public::routes as public_routes;

pub(crate) mod db;
pub mod error;
pub mod pingtask;
pub(crate) mod schema;
pub(crate) mod servers;
pub mod state;
pub(crate) mod views;

pub async fn serve(routes: Router<AppState>, addr: SocketAddr) -> error::Result<()> {
	let service = routes
		.with_state(AppState::init()?)
		.layer(
			TraceLayer::new_for_http()
				.make_span_with(|request: &http::Request<_>| {
					tracing::info_span!(
						"http",
						req.version = ?request.version(),
						req.uri = %request.uri(),
						req.method = %request.method(),
						res.version = tracing::field::Empty,
						res.status = tracing::field::Empty,
						latency = tracing::field::Empty,
					)
				})
				.on_response(
					|response: &http::Response<_>, latency: Duration, span: &Span| {
						span.record("res.version", &format!("{:?}", response.version()));
						span.record("res.status", &response.status().as_u16());
						span.record("latency", &format!("{:?}", latency));
						tracing::info!("response");
					},
				),
		)
		.into_make_service();

	let listener = TcpListener::bind(addr).await?;
	tracing::info!("listening on {}", listener.local_addr()?);
	axum::serve(listener, service).await?;
	Ok(())
}
