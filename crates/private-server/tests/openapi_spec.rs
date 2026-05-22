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
	assert!(
		schemes["tailscale-admin"].is_object(),
		"tailscale-admin scheme present"
	);
	assert!(
		schemes["tailscale-user"].is_object(),
		"tailscale-user scheme present"
	);
}

#[test]
fn spec_has_problem_details_schema() {
	let spec = build_spec();
	let schema = &spec["components"]["schemas"]["ProblemDetailsSchema"];
	assert!(schema.is_object(), "ProblemDetailsSchema is registered");
}

/// Each module gets a representative path checked.
#[test]
fn spec_has_paths_for_every_module() {
	let spec = build_spec();
	let paths = &spec["paths"];
	for p in [
		"/api/admins/list",
		"/api/bestool/list_snippets",
		"/api/commons/public_url",
		"/api/devices/list_trusted",
		"/api/incidents/list_active",
		"/api/issues/list",
		"/api/server_groups/list",
		"/api/sql/is_sql_available",
		"/api/statuses/summary",
		"/api/versions/get_grouped_versions",
	] {
		assert!(paths[p].is_object(), "{p} present in spec");
	}
}

/// Drift-check: the committed `private-web/openapi.json` must match what the
/// current rust handlers produce. If this fails, run `just gen-openapi` and
/// commit the resulting diff.
#[test]
fn committed_spec_matches_generated() {
	let path = concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../private-web/openapi.json"
	);
	let raw = std::fs::read_to_string(path)
		.expect("read private-web/openapi.json — generate it with `just gen-openapi`");
	let committed: serde_json::Value =
		serde_json::from_str(&raw).expect("private-web/openapi.json is valid JSON");
	let generated = build_spec();
	assert!(
		committed == generated,
		"private-web/openapi.json is stale — run `just gen-openapi` to refresh and commit the diff",
	);
}

#[test]
fn spec_path_count() {
	let spec = build_spec();
	let paths = spec["paths"].as_object().expect("paths is an object");
	// Sanity bound: every handler in private-server is annotated. If this drops
	// far below ~70, someone removed annotations; if it climbs much higher,
	// new endpoints exist that should be reviewed.
	assert!(
		paths.len() >= 60,
		"expected >=60 paths in spec, got {}",
		paths.len()
	);
}
