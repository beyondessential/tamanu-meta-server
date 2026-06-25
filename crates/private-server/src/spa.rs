use axum::body::Body;
use axum::http::{HeaderValue, Response, StatusCode, Uri, header};
use axum::response::IntoResponse;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../private-web/dist/"]
struct Assets;

/// Fallback handler that serves the embedded React SPA. Static assets are
/// served from their hashed path with long-lived cache; everything else
/// falls back to `index.html` so the client-side router can take over.
pub async fn handler(uri: Uri) -> impl IntoResponse {
	let path = uri.path().trim_start_matches('/');
	let is_asset = path.starts_with("assets/");

	// Don't serve the SPA for `/.well-known/` probes. MCP clients (e.g. Claude
	// Code) discover OAuth by fetching `/.well-known/oauth-protected-resource`;
	// answering with `index.html` makes them fail with "Failed to parse JSON"
	// instead of concluding there's no OAuth. A 404 means "not a protected
	// resource — connect without OAuth" (this endpoint authenticates via the
	// Tailscale ingress, not OAuth).
	if path.starts_with(".well-known/") {
		return (StatusCode::NOT_FOUND, "not found").into_response();
	}

	let resolved = if !is_asset && Assets::get(path).is_none() {
		"index.html"
	} else {
		path
	};

	let Some(file) = Assets::get(resolved) else {
		return (StatusCode::NOT_FOUND, "not found").into_response();
	};

	let mime_type = mime_guess::from_path(resolved).first_or_octet_stream();
	let mime = if mime_type.type_() == mime_guess::mime::TEXT {
		format!("{mime_type}; charset=utf-8")
	} else {
		mime_type.to_string()
	};

	let cache = if is_asset {
		HeaderValue::from_static("public, max-age=31536000, immutable")
	} else {
		HeaderValue::from_static("no-cache, no-store, must-revalidate")
	};

	Response::builder()
		.status(StatusCode::OK)
		.header(header::CONTENT_TYPE, mime)
		.header(header::CACHE_CONTROL, cache)
		.body(Body::from(file.data.into_owned()))
		.unwrap()
}
