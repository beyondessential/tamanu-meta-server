//! Smoke test: the OpenAPI spec generates and exposes one path per migrated module.

use private_server::openapi::ApiDoc;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

fn build_spec() -> serde_json::Value {
	let (_router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
		.merge(private_server::fns::routes())
		.split_for_parts();
	serde_json::to_value(&openapi).expect("openapi serializes")
}

#[test]
fn spec_has_security_schemes() {
	let spec = build_spec();
	let schemes = &spec["components"]["securitySchemes"];
	assert!(schemes["tailscale-admin"].is_object(), "tailscale-admin scheme present");
	assert!(schemes["tailscale-user"].is_object(), "tailscale-user scheme present");
}

#[test]
fn spec_has_problem_details_schema() {
	let spec = build_spec();
	let schema = &spec["components"]["schemas"]["ProblemDetailsSchema"];
	assert!(schema.is_object(), "ProblemDetailsSchema is registered");
}

/// Each module is checked by asserting at least one of its routes appears in
/// the generated paths. As modules are annotated they're added here.
#[test]
fn spec_has_admin_paths() {
	let spec = build_spec();
	let paths = &spec["paths"];
	for p in ["/api/admins/list", "/api/admins/add", "/api/admins/delete"] {
		assert!(paths[p].is_object(), "{p} present in spec");
	}
}
