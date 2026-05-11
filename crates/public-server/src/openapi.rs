use utoipa::{
	Modify, OpenApi,
	openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};

/// Base OpenAPI document. Path entries are pulled in by `utoipa-axum`'s
/// `OpenApiRouter` as routes are registered. HTML, redirect, and binary-stream
/// endpoints (the UI side, the `timesync` Timesimp protocol, the artifact
/// download proxy) are deliberately excluded from the spec.
#[derive(OpenApi)]
#[openapi(
	info(
		title = "canopy public-server",
		description = "Internet-facing API for the canopy fleet. Device-authenticated endpoints require an mTLS client certificate carried through the edge proxy as the `x-forwarded-client-cert` (XFCC) header, with `mtls-certificate` and `ssl-client-cert` accepted as fallbacks; the certificate's public key is matched against the device registry and its role (server / releaser / admin) gates access.",
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
		// OpenAPI 3.1 has no first-class mTLS scheme. The actual transport is
		// mTLS terminated at the edge proxy, which forwards the client cert
		// as an HTTP header; we model that header as an apiKey scheme.
		let scheme_for = |role: &str, extra: &str| -> SecurityScheme {
			let description = format!(
				"Envoy-style XFCC header carrying the device's mTLS client certificate. {extra} The device's role is read from the registry on each request; {role} role (or admin) is required.",
			);
			SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
				"x-forwarded-client-cert",
				&description,
			)))
		};
		components.add_security_scheme(
			"server-device",
			scheme_for(
				"server",
				"Devices register themselves as `server` role on first contact.",
			),
		);
		components.add_security_scheme(
			"releaser-device",
			scheme_for(
				"releaser",
				"Releaser certificates are issued out-of-band to CI / release machines.",
			),
		);
		components.add_security_scheme(
			"admin-device",
			scheme_for(
				"admin",
				"Admin certificates are issued out-of-band to operators.",
			),
		);
	}
}
