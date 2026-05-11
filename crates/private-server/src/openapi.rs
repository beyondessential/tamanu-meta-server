use utoipa::{
	Modify, OpenApi,
	openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};

/// Base OpenAPI document. Concrete paths and schemas are pulled in by
/// `utoipa-axum`'s `OpenApiRouter` as routes are registered.
#[derive(OpenApi)]
#[openapi(
	info(
		title = "canopy private-server",
		description = "Admin/operator API for the canopy fleet manager. Requests are gated behind Tailscale auth — every request must carry the `Tailscale-User-Login` header injected by the Tailscale sidecar; admin-only endpoints additionally check the caller is on the admin list.",
		license(name = "GPL-3.0-or-later"),
	),
	modifiers(&SecuritySchemes),
	tags(
		(name = "admins", description = "Admin email allow-list management."),
		(name = "bestool", description = "Bestool SQL snippet library."),
		(name = "commons", description = "Shared configuration and identity helpers."),
		(name = "devices", description = "Device registry, trust, and key management."),
		(name = "incidents", description = "Operational incidents (groups of issues against a server)."),
		(name = "issues", description = "Per-server issues raised from device events."),
		(name = "servers", description = "Server inventory, hierarchy, and metadata."),
		(name = "sql", description = "Read-only SQL playground."),
		(name = "statuses", description = "Live server status and version-distance summaries."),
		(name = "versions", description = "Canopy release versions and downloadable artifacts."),
	),
)]
pub struct ApiDoc;

struct SecuritySchemes;

impl Modify for SecuritySchemes {
	fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
		let components = openapi.components.get_or_insert_with(Default::default);
		components.add_security_scheme(
			"tailscale-user",
			SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
				"Tailscale-User-Login",
				"Identity header injected by the Tailscale sidecar. Required for any caller-identifying endpoint.",
			))),
		);
		components.add_security_scheme(
			"tailscale-admin",
			SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
				"Tailscale-User-Login",
				"Identity header injected by the Tailscale sidecar. The login value must also be present on the admins allow-list.",
			))),
		);
	}
}
