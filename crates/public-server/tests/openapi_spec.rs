//! Smoke test: the public-server OpenAPI spec generates and exposes the
//! handlers we expect. Mirrors the equivalent test in `private-server`.

use public_server::openapi::ApiDoc;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

fn build_spec() -> serde_json::Value {
	let (_router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
		.merge(public_server::routes())
		.split_for_parts();
	serde_json::to_value(&openapi).expect("openapi serializes")
}

#[test]
fn spec_has_security_schemes() {
	let spec = build_spec();
	let schemes = &spec["components"]["securitySchemes"];
	for s in ["server-device", "releaser-device", "admin-device"] {
		assert!(schemes[s].is_object(), "{s} scheme present");
	}
}

#[test]
fn spec_has_problem_details_schema() {
	let spec = build_spec();
	let schema = &spec["components"]["schemas"]["ProblemDetailsSchema"];
	assert!(schema.is_object(), "ProblemDetailsSchema is registered");
}

#[test]
fn spec_has_one_path_per_module() {
	let spec = build_spec();
	let paths = &spec["paths"];
	for p in [
		"/events",
		"/servers",
		"/bestool/snippets",
		"/status/{server_id}",
		"/versions",
		"/versions/update-for/{version}",
		"/versions/{version}",
		"/versions/{version}/artifacts",
		"/artifacts/{version}/{artifact_type}/{platform}",
	] {
		assert!(paths[p].is_object(), "{p} present in spec");
	}
}

#[test]
fn spec_excludes_html_and_streaming_routes() {
	let spec = build_spec();
	let paths = &spec["paths"];
	// HTML view, mobile install, and streamed binary proxy must not appear.
	assert!(
		paths["/versions/{version}/artifacts/{artifact_id}/download"].is_null(),
		"download proxy is excluded"
	);
	// `/{version}` GET is the HTML view (cfg ui); the spec only knows the
	// JSON post + delete on the same path. Spot-check that the spec lists
	// post and delete but not get.
	let methods = &paths["/versions/{version}"]
		.as_object()
		.expect("path object");
	assert!(methods.contains_key("post"), "post present");
	assert!(methods.contains_key("delete"), "delete present");
	assert!(!methods.contains_key("get"), "html view get not in spec");
}
