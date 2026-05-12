//! Dump the public-server's OpenAPI spec to stdout as pretty-printed JSON.
//!
//! Used by `just gen-openapi` to refresh `crates/public-server/openapi.json`.
//! No database or network is required — the spec is fully derived from
//! compile-time annotations.

use public_server::openapi::ApiDoc;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

fn main() {
	let (_router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
		.merge(public_server::routes())
		.split_for_parts();
	let json = serde_json::to_string_pretty(&openapi).expect("serialize spec");
	println!("{json}");
}
