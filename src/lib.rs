use std::{net::SocketAddr, time::Duration};

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use axum::{Router, middleware};
use axum_client_ip::{ClientIp, ClientIpSource};
use axum_server_timing::ServerTimingLayer;
use tokio::net::TcpListener;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::Span;

pub use servers::private::routes as private_routes;
pub use servers::public::routes as public_routes;

pub mod db;
pub mod error;
pub mod migrator;
pub mod ownstatus;
pub mod pingtask;
pub(crate) mod schema;
pub(crate) mod servers;
pub mod state;
pub(crate) mod views;

pub fn router(routes: Router<()>, client_ip_source: ClientIpSource) -> Router<()> {
	routes
		// ordering of the client ip middlewares is critical, do not change
		.layer(middleware::from_fn(ip_into_response))
		.layer(client_ip_source.into_extension())
		.layer(
			TraceLayer::new_for_http()
				.make_span_with(|request: &http::Request<_>| {
					tracing::info_span!(
						"http",
						req.version = ?request.version(),
						req.uri = %request.uri(),
						req.method = %request.method(),
						req.ip = tracing::field::Empty,
						res.version = tracing::field::Empty,
						res.status = tracing::field::Empty,
						latency = tracing::field::Empty,
					)
				})
				.on_response(
					|response: &http::Response<_>, latency: Duration, span: &Span| {
						if let Some(ip) = response.extensions().get::<ClientIp>().map(|r| &r.0) {
							span.record("req.ip", tracing::field::debug(ip));
						}

						span.record("latency", tracing::field::debug(latency));
						span.record("res.version", tracing::field::debug(response.version()));
						span.record(
							"res.status",
							tracing::field::display(response.status().as_u16()),
						);
						tracing::info!("response");
					},
				),
		)
		.layer(CompressionLayer::new())
		.layer(ServerTimingLayer::new("srv"))
}

pub async fn serve(routes: Router<()>, addr: SocketAddr) -> error::Result<()> {
	let service = routes.into_make_service_with_connect_info::<SocketAddr>();
	let listener = TcpListener::bind(addr).await?;
	tracing::info!("listening on {}", listener.local_addr()?);
	axum::serve(listener, service).await?;
	Ok(())
}

async fn ip_into_response(ip: ClientIp, request: Request, next: Next) -> Response {
	tracing::trace!(?ip, "ip_into_response middleware");
	let mut response = next.run(request).await;
	response.extensions_mut().insert(ip);
	response
}
