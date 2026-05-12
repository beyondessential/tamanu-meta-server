use utoipa::{Modify, OpenApi, openapi::security::SecurityScheme};

/// Base OpenAPI document. Path entries are pulled in by `utoipa-axum`'s
/// `OpenApiRouter` as routes are registered. HTML, redirect, and binary-stream
/// endpoints (the UI side, the `timesync` Timesimp protocol, the artifact
/// download proxy) are deliberately excluded from the spec.
#[derive(OpenApi)]
#[openapi(
	info(
		title = "canopy public-server",
		description = "Internet-facing API for the canopy fleet. Device-authenticated endpoints require an mTLS client certificate.",
		contact(name = "BES Developers", email = "contact@bes.au"),
		license(name = "GPL-3.0-or-later"),
	),
	modifiers(&SecuritySchemes),
	tags(
		(name = "artifacts", description = "Per-version artifact registration by releaser devices."),
		(name = "bestool", description = "Bestool SQL snippet read API."),
		(name = "events", description = "Device-pushed events; rolled up into issues and incidents server-side."),
		(name = "servers", description = "Server registry — listing for the public, self-registration for server devices."),
		(name = "statuses", description = "Heartbeat / status submissions from server devices."),
		(name = "versions", description = "Canopy release versions and their downloadable artifacts."),
	),
)]
pub struct ApiDoc;

struct SecuritySchemes;

impl Modify for SecuritySchemes {
	fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
		let components = openapi.components.get_or_insert_with(Default::default);
		// One mTLS scheme per role so individual endpoints can name what they
		// require; the actual transport (mTLS terminated at the edge proxy,
		// cert keyed against the device registry) is an implementation
		// detail callers don't need.
		let role_scheme = |role: &str| -> SecurityScheme {
			SecurityScheme::MutualTls {
				description: Some(format!(
					"mTLS client certificate for a device with the `{role}` role (or `admin`).",
				)),
				extensions: None,
			}
		};
		components.add_security_scheme("server-device", role_scheme("server"));
		components.add_security_scheme("releaser-device", role_scheme("releaser"));
		components.add_security_scheme(
			"admin-device",
			SecurityScheme::MutualTls {
				description: Some(
					"mTLS client certificate for a device with the `admin` role.".to_string(),
				),
				extensions: None,
			},
		);
	}
}
