//! mTLS path of the device-auth extractor. Reads a client certificate
//! from one of the supported headers, derives a stable public-key blob,
//! and resolves it to an existing [`Device`] row.
//!
//! A device row is created only through the gated enrollment flow
//! (`/servers/register/*`); an unknown key here is `AuthCertificateNotFound`.

use std::sync::LazyLock;

use commons_errors::{AppError, Result};
use database::devices::Device;
use diesel_async::AsyncPgConnection;
use http::request::Parts;
use tracing::warn;
use x509_parser::prelude::*;

/// Env var gating the nginx path (`mtls-certificate` / `ssl-client-cert`).
/// Default **on** — this is what the live ingress sets today.
const MTLS_HEADER_ENV: &str = "CANOPY_DEVICE_AUTH_MTLS_HEADER";
/// Env var gating the Envoy path (`x-forwarded-client-cert`).
/// Default **off** — see [`TrustedCertHeaders`].
const XFCC_ENV: &str = "CANOPY_DEVICE_AUTH_XFCC";

/// Which client-certificate headers this deployment trusts.
///
/// A header naming a client certificate is only meaningful if it can *only*
/// have been set by the proxy that terminated the TLS connection and verified
/// the peer. [`resolve`] authenticates by looking the certificate's public key
/// up in `devices` — there is no proof of possession at this layer, and a
/// public key is not a secret — so any caller able to set a trusted header can
/// present an enrolled device's certificate and be resolved as that device.
///
/// The two ingress paths are therefore gated separately, and only the one a
/// deployment actually runs should be on:
///
/// - **nginx** (`mtls-certificate`, `ssl-client-cert`) — on by default; the
///   live path today.
/// - **Envoy** (`x-forwarded-client-cert`) — off by default. Envoy is not yet
///   deployed, and nginx has no reason to strip a header it doesn't use, so
///   accepting XFCC unconditionally meant a client could set it themselves.
///   It was also *preferred* over the nginx header, so it overrode the
///   genuinely-verified certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustedCertHeaders {
	/// Trust `mtls-certificate` / `ssl-client-cert`.
	pub mtls_header: bool,
	/// Trust `x-forwarded-client-cert`.
	pub xfcc: bool,
}

impl Default for TrustedCertHeaders {
	fn default() -> Self {
		Self {
			mtls_header: true,
			xfcc: false,
		}
	}
}

impl TrustedCertHeaders {
	fn from_env() -> Self {
		let default = Self::default();
		let me = Self {
			mtls_header: env_flag(MTLS_HEADER_ENV, default.mtls_header),
			xfcc: env_flag(XFCC_ENV, default.xfcc),
		};
		if !me.mtls_header && !me.xfcc {
			warn!(
				"both {MTLS_HEADER_ENV} and {XFCC_ENV} are off: no client-certificate header \
				 is trusted, so mTLS device auth cannot succeed"
			);
		}
		if me.mtls_header && me.xfcc {
			warn!(
				"both {MTLS_HEADER_ENV} and {XFCC_ENV} are on: only the ingress actually in \
				 front of this server should be trusted, or the other header can be spoofed"
			);
		}
		me
	}
}

/// Parse a boolean env var, falling back to `default` when unset, empty, or
/// unrecognised. An unrecognised value warns rather than silently flipping a
/// security-relevant switch.
fn env_flag(key: &str, default: bool) -> bool {
	let Ok(raw) = std::env::var(key) else {
		return default;
	};
	match raw.trim().to_ascii_lowercase().as_str() {
		"" => default,
		"1" | "true" | "yes" | "on" => true,
		"0" | "false" | "no" | "off" => false,
		other => {
			warn!("{key}: unrecognised value {other:?}, keeping the default ({default})");
			default
		}
	}
}

static TRUSTED: LazyLock<TrustedCertHeaders> = LazyLock::new(TrustedCertHeaders::from_env);

/// Resolve a request to a [`Device`] via mTLS. Returns:
///
/// - `Ok(Some(device))` — cert present and its key matches a known device.
/// - `Ok(None)` — no cert header at all (caller may try another path).
/// - `Err(AuthCertificateNotFound)` — cert present but its key is unknown.
/// - `Err(_)` — header present but malformed.
pub async fn resolve(parts: &Parts, db: &mut AsyncPgConnection) -> Result<Option<Device>> {
	let Some(key) = spki_from_headers(&parts.headers)? else {
		return Ok(None);
	};

	match Device::from_key(db, &key).await? {
		Some(existing) => Ok(Some(existing)),
		None => Err(AppError::AuthCertificateNotFound),
	}
}

/// Extract the presented client certificate's `SubjectPublicKeyInfo` (DER)
/// from the request headers, *without* touching the database or creating a
/// device. Returns `Ok(None)` when no cert header is present. The enrollment
/// endpoints use this to own device resolution themselves.
pub fn spki_from_headers(headers: &http::HeaderMap) -> Result<Option<Vec<u8>>> {
	let Some(pem) = extract_cert_pem(headers)? else {
		return Ok(None);
	};

	let (_, der) = parse_x509_pem(pem.as_bytes())
		.map_err(|e| AppError::AuthInvalidCertificate(format!("Invalid PEM format: {}", e)))?;
	let (_, cert) = parse_x509_certificate(&der.contents).map_err(|e| {
		AppError::AuthInvalidCertificate(format!("Invalid X.509 certificate: {}", e))
	})?;

	Ok(Some(cert.tbs_certificate.subject_pki.raw.to_vec()))
}

fn extract_cert_pem(headers: &http::HeaderMap) -> Result<Option<String>> {
	extract_cert_pem_with(headers, *TRUSTED)
}

/// [`extract_cert_pem`] against an explicit trust configuration, so the
/// gating is testable without touching process-wide env.
fn extract_cert_pem_with(
	headers: &http::HeaderMap,
	trusted: TrustedCertHeaders,
) -> Result<Option<String>> {
	// Prefer x-forwarded-client-cert (Envoy XFCC format) when trusted and
	// present, falling back to mtls-certificate and ssl-client-cert headers.
	let xfcc_cert = trusted
		.xfcc
		.then(|| {
			headers
				.get("x-forwarded-client-cert")
				.and_then(|v| v.to_str().ok())
				.and_then(xfcc_client_cert)
		})
		.flatten();

	if let Some(cert_value) = xfcc_cert {
		return Ok(Some(
			percent_encoding::percent_decode(cert_value.as_bytes())
				.decode_utf8()
				.map_err(|e| {
					AppError::AuthInvalidCertificate(format!("Invalid UTF-8 in certificate: {}", e))
				})?
				.into_owned(),
		));
	}

	if !trusted.mtls_header {
		return Ok(None);
	}
	let Some(value) = headers
		.get("mtls-certificate")
		.or_else(|| headers.get("ssl-client-cert"))
	else {
		return Ok(None);
	};

	Ok(Some(
		percent_encoding::percent_decode(value.as_bytes())
			.decode_utf8()
			.map_err(|e| {
				AppError::AuthInvalidCertificate(format!("Invalid UTF-8 in certificate: {}", e))
			})?
			.into_owned(),
	))
}

/// The `Cert=` value describing the TLS peer of the proxy that terminated
/// *our* connection, from an `x-forwarded-client-cert` header.
///
/// Each trusted proxy in a chain **appends** its element, so the element we
/// may trust is the **last** one — everything before it describes hops further
/// upstream and, at the head of the list, whatever the original client chose
/// to send. Reading the first element lets a caller present the public
/// certificate of any device (certificates are not secret) and be
/// authenticated as it.
///
/// Returns `None` if the last element carries no `Cert=`, rather than
/// searching backwards: an earlier element is exactly the untrusted input this
/// exists to ignore.
fn xfcc_client_cert(header: &str) -> Option<&str> {
	let last = split_unquoted(header, ',').last()?;
	split_unquoted(last, ';')
		.find_map(|field| field.trim().strip_prefix("Cert="))
		.map(unquote)
}

/// Split on `delim`, ignoring delimiters inside double-quoted values. Envoy
/// quotes any XFCC value containing the separators, so a naive split can cut
/// an element in half — and mis-slicing the element list is what decides which
/// hop we trust.
fn split_unquoted(s: &str, delim: char) -> impl Iterator<Item = &str> {
	let mut in_quotes = false;
	let mut escaped = false;
	let mut start = 0;
	let mut out = Vec::new();
	for (i, c) in s.char_indices() {
		match c {
			_ if escaped => escaped = false,
			'\\' if in_quotes => escaped = true,
			'"' => in_quotes = !in_quotes,
			_ if c == delim && !in_quotes => {
				out.push(&s[start..i]);
				start = i + c.len_utf8();
			}
			_ => {}
		}
	}
	out.push(&s[start..]);
	out.into_iter()
}

/// Strip the surrounding quotes Envoy adds to values containing separators,
/// leaving an unquoted value untouched. Only the quoting is undone here; the
/// value is still percent-encoded.
fn unquote(value: &str) -> &str {
	value
		.strip_prefix('"')
		.and_then(|v| v.strip_suffix('"'))
		.unwrap_or(value)
}

#[cfg(test)]
mod tests {
	use super::*;

	const A: &str = "-----BEGIN%20CERTIFICATE-----A";
	const B: &str = "-----BEGIN%20CERTIFICATE-----B";

	#[test]
	fn single_element_uses_its_cert() {
		assert_eq!(
			xfcc_client_cert(&format!("By=spiffe://mesh/ingress;Hash=abc;Cert={A}")),
			Some(A),
		);
	}

	/// The attack: an attacker-supplied element arrives first and the
	/// terminating proxy appends its own. Trusting the first authenticates the
	/// attacker as whichever device's (public, non-secret) certificate they
	/// pasted in.
	#[test]
	fn chained_elements_use_the_last_cert() {
		assert_eq!(
			xfcc_client_cert(&format!("Cert={A},By=spiffe://mesh/ingress;Cert={B}")),
			Some(B),
			"the element appended by the terminating proxy is the last one",
		);
	}

	#[test]
	fn a_last_element_without_a_cert_yields_nothing() {
		assert_eq!(
			xfcc_client_cert(&format!("Cert={A},By=spiffe://mesh/ingress;Hash=abc")),
			None,
			"falling back to an earlier element would trust upstream input",
		);
	}

	/// Envoy quotes values containing separators. A naive split on `,` would
	/// cut this element in two and take the quoted tail as the last element.
	#[test]
	fn quoted_values_do_not_split_elements() {
		assert_eq!(
			xfcc_client_cert(&format!(r#"By="spiffe://mesh/a,b";Cert={A}"#)),
			Some(A),
		);
		assert_eq!(
			xfcc_client_cert(&format!(r#"By="a;b";Cert={A},By="c,d";Cert={B}"#)),
			Some(B),
		);
	}

	#[test]
	fn a_quoted_cert_value_is_unquoted() {
		assert_eq!(
			xfcc_client_cert(&format!(r#"By=ingress;Cert="{A}""#)),
			Some(A),
		);
	}

	#[test]
	fn an_empty_header_yields_nothing() {
		assert_eq!(xfcc_client_cert(""), None);
	}

	// --- header trust gating ---

	const PEM: &str = "-----BEGIN%20CERTIFICATE-----A";

	fn headers(pairs: &[(&str, &str)]) -> http::HeaderMap {
		let mut h = http::HeaderMap::new();
		for (k, v) in pairs {
			h.insert(
				http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
				http::HeaderValue::from_str(v).unwrap(),
			);
		}
		h
	}

	fn extracted(pairs: &[(&str, &str)], trusted: TrustedCertHeaders) -> Option<String> {
		extract_cert_pem_with(&headers(pairs), trusted).expect("well-formed headers")
	}

	/// The shipped default: nginx's header is honoured, Envoy's is not.
	#[test]
	fn defaults_trust_the_nginx_header_only() {
		let d = TrustedCertHeaders::default();
		assert!(d.mtls_header && !d.xfcc);
	}

	/// The bug this gate exists for. Envoy is not deployed, and nginx has no
	/// reason to strip a header it doesn't use, so an untrusted caller could
	/// set XFCC itself — and it was *preferred* over the header nginx
	/// actually verifies.
	#[test]
	fn xfcc_is_ignored_by_default_even_when_present() {
		assert_eq!(
			extracted(
				&[("x-forwarded-client-cert", &format!("Cert={PEM}"))],
				TrustedCertHeaders::default(),
			),
			None,
			"an untrusted XFCC header must not authenticate anything",
		);
	}

	/// And it must not be able to override the genuinely-verified one.
	#[test]
	fn untrusted_xfcc_cannot_override_the_nginx_header() {
		let nginx = "-----BEGIN%20CERTIFICATE-----NGINX";
		assert_eq!(
			extracted(
				&[
					("x-forwarded-client-cert", &format!("Cert={PEM}")),
					("mtls-certificate", nginx),
				],
				TrustedCertHeaders::default(),
			)
			.as_deref(),
			Some("-----BEGIN CERTIFICATE-----NGINX"),
		);
	}

	#[test]
	fn xfcc_is_honoured_once_enabled() {
		assert_eq!(
			extracted(
				&[("x-forwarded-client-cert", &format!("Cert={PEM}"))],
				TrustedCertHeaders {
					mtls_header: false,
					xfcc: true,
				},
			)
			.as_deref(),
			Some("-----BEGIN CERTIFICATE-----A"),
		);
	}

	/// Turning the nginx path off must actually stop it being read — that's
	/// the whole point of the switch for the eventual Envoy cutover.
	#[test]
	fn the_nginx_header_is_ignored_when_disabled() {
		for header in ["mtls-certificate", "ssl-client-cert"] {
			assert_eq!(
				extracted(
					&[(header, PEM)],
					TrustedCertHeaders {
						mtls_header: false,
						xfcc: true,
					},
				),
				None,
				"{header} must not be read once disabled",
			);
		}
	}

	#[test]
	fn nothing_is_read_when_both_are_off() {
		let off = TrustedCertHeaders {
			mtls_header: false,
			xfcc: false,
		};
		assert_eq!(
			extracted(
				&[
					("x-forwarded-client-cert", &format!("Cert={PEM}")),
					("mtls-certificate", PEM),
				],
				off,
			),
			None,
		);
	}

	#[test]
	fn env_flag_parses_both_spellings_and_keeps_the_default_otherwise() {
		// SAFETY: single-threaded test, and the key is unique to it.
		let key = "CANOPY_TEST_DEVICE_AUTH_FLAG";
		for (raw, expected) in [
			("1", true),
			("true", true),
			("ON", true),
			(" yes ", true),
			("0", false),
			("false", false),
			("Off", false),
			("no", false),
		] {
			unsafe { std::env::set_var(key, raw) };
			assert_eq!(env_flag(key, !expected), expected, "{raw:?}");
		}
		// Unrecognised and empty both keep the default rather than flipping
		// a security switch on a typo.
		for raw in ["", "  ", "maybe"] {
			unsafe { std::env::set_var(key, raw) };
			assert!(env_flag(key, true), "{raw:?} should keep default true");
			assert!(!env_flag(key, false), "{raw:?} should keep default false");
		}
		unsafe { std::env::remove_var(key) };
		assert!(env_flag(key, true));
		assert!(!env_flag(key, false));
	}
}
