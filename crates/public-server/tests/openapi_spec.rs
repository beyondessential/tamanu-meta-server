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
	for s in [
		"server-device",
		"releaser-device",
		"admin-device",
		"backup-restore-device",
	] {
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

/// Drift-check: the committed `crates/public-server/openapi.json` must match
/// what the current rust handlers produce. If this fails, run
/// `just gen-openapi` and commit the resulting diff.
#[test]
fn committed_spec_matches_generated() {
	let path = concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json");
	let raw = std::fs::read_to_string(path)
		.expect("read crates/public-server/openapi.json — generate it with `just gen-openapi`");
	let committed: serde_json::Value =
		serde_json::from_str(&raw).expect("crates/public-server/openapi.json is valid JSON");
	let generated = build_spec();
	assert!(
		committed == generated,
		"crates/public-server/openapi.json is stale — run `just gen-openapi` to refresh and commit the diff",
	);
}

/// `operationId` must be unique across the whole document (OpenAPI requires it,
/// and codegen/tooling keys off it). utoipa defaults an operation's id to the
/// handler's function name, so two same-named handlers in different modules
/// (e.g. several `create`s) silently collide — set an explicit `operation_id`
/// in the `#[utoipa::path]` to disambiguate.
#[test]
fn no_duplicate_operation_ids() {
	let spec = build_spec();
	let paths = spec["paths"].as_object().expect("paths object");

	let mut by_id: std::collections::BTreeMap<String, Vec<String>> = Default::default();
	for (path, item) in paths {
		let ops = item.as_object().expect("path item object");
		for (method, op) in ops {
			// Non-operation keys (parameters, summary, …) aren't objects with
			// an operationId, so they fall through this filter.
			if let Some(id) = op.get("operationId").and_then(|v| v.as_str()) {
				by_id
					.entry(id.to_string())
					.or_default()
					.push(format!("{} {path}", method.to_uppercase()));
			}
		}
	}

	let dupes: Vec<String> = by_id
		.iter()
		.filter(|(_, uses)| uses.len() > 1)
		.map(|(id, uses)| format!("{id}: {}", uses.join(", ")))
		.collect();
	assert!(
		dupes.is_empty(),
		"duplicate operationIds (invalid OpenAPI) — set an explicit `operation_id`:\n{}",
		dupes.join("\n"),
	);
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
