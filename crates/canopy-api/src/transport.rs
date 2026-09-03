//! The HTTP layer underneath [`CanopyClient`](crate::CanopyClient).
//!
//! Everything above this layer (the wire types, the per-endpoint methods,
//! gzipping, status handling, JSON parsing) is transport-agnostic: this crate
//! does not decide how a request reaches canopy, and depends on no HTTP client.
//! A consumer implements [`CanopyTransport`] and keeps the whole typed interface
//! on top of it.

use std::sync::Arc;

use bytes::Bytes;

use crate::Result;

/// A request built by [`CanopyClient`](crate::CanopyClient), ready for a
/// [`CanopyTransport`] to send.
///
/// The URI is the endpoint **path** in origin form (path plus query, no scheme
/// or authority), e.g. `/backup-target` — resolving it against a base URL is the
/// transport's job. The body is already serialised and gzipped when there is one
/// (with `content-type` and `content-encoding` set to match) and empty when
/// there isn't.
pub type CanopyRequest = http::Request<Bytes>;

/// A response handed back to [`CanopyClient`](crate::CanopyClient) by a
/// [`CanopyTransport`], with its body buffered.
///
/// The status is interpreted by the client: a non-2xx becomes a
/// [`CanopyHttpError`](crate::CanopyHttpError) carrying the body, and a success
/// has its body parsed into the endpoint's response type.
pub type CanopyResponse = http::Response<Bytes>;

/// The HTTP transport a [`CanopyClient`](crate::CanopyClient) sends through.
///
/// Implement this to route canopy calls through whatever reaches canopy from
/// where you are — an mTLS client, a tailnet address, a proxy that isn't a plain
/// HTTP proxy, an in-process handler, a recorded fixture in tests — and pass it
/// to [`CanopyClient::new`](crate::CanopyClient::new). The per-endpoint methods,
/// the wire types, and the error handling all work unchanged on top.
///
/// # Contract
///
/// - Requests arrive with a path-only URI (see [`CanopyRequest`]); the transport
///   decides what host, scheme, and authentication to use, and may rewrite the
///   path.
/// - Return canopy's response as-is, non-2xx included: statuses are the client's
///   to interpret, since endpoints give meaning to specific codes.
/// - [`Err`] is for a failure to obtain any response at all (connect, timeout,
///   protocol error), which is distinct from a response reporting failure.
#[async_trait::async_trait]
pub trait CanopyTransport: Send + Sync {
	/// Send `request` and return canopy's response.
	async fn call(&self, request: CanopyRequest) -> Result<CanopyResponse>;
}

#[async_trait::async_trait]
impl<T: CanopyTransport + ?Sized> CanopyTransport for Arc<T> {
	async fn call(&self, request: CanopyRequest) -> Result<CanopyResponse> {
		(**self).call(request).await
	}
}

#[async_trait::async_trait]
impl<T: CanopyTransport + ?Sized> CanopyTransport for Box<T> {
	async fn call(&self, request: CanopyRequest) -> Result<CanopyResponse> {
		(**self).call(request).await
	}
}
