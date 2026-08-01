//! mTLS path of the device-auth extractor. Reads a client certificate
//! from one of the supported headers, derives a stable public-key blob,
//! and resolves it to an existing [`Device`] row.
//!
//! A device row is created only through the gated enrollment flow
//! (`/servers/register/*`); an unknown key here is `AuthCertificateNotFound`.

use commons_errors::{AppError, Result};
use database::devices::Device;
use diesel_async::AsyncPgConnection;
use http::request::Parts;
use tracing::warn;
use x509_parser::prelude::*;

/// Env var naming the client-certificate header to trust. See
/// [`ClientCertHeader`].
const CERT_HEADER_ENV: &str = "CANOPY_DEVICE_AUTH_CERT_HEADER";

/// Which client-certificate header this deployment's ingress sets — and
/// therefore the only one that may be believed.
///
/// A header naming a client certificate is meaningful only if it can *only*
/// have been set by the proxy that terminated the TLS connection and verified
/// the peer. [`resolve`] authenticates by looking the certificate's public key
/// up in `devices`: there is no proof of possession at this layer, and a
/// public key is not a secret. So any caller able to set a believed header can
/// present an enrolled device's certificate and be resolved as that device.
///
/// One setting, naming one header, so "trust both" and "trust neither" are
/// not states this can be configured into. XFCC used to be read
/// unconditionally *and* preferred over the nginx header — Envoy is not
/// deployed, and nginx has no reason to strip a header it doesn't use, so a
/// client could set it themselves and override the genuinely-verified
/// certificate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClientCertHeader {
	/// nginx: `mtls-certificate`, falling back to `ssl-client-cert`. The live
	/// ingress path, and the default.
	#[default]
	Mtls,
	/// Envoy: `x-forwarded-client-cert`. Select this when the Envoy ingress
	/// is what fronts this server.
	Xfcc,
}

impl ClientCertHeader {
	/// Read the configured header from the environment. Called once when a
	/// server's state is built.
	pub fn from_env() -> Self {
		let Ok(raw) = std::env::var(CERT_HEADER_ENV) else {
			return Self::default();
		};
		match raw.trim().to_ascii_lowercase().as_str() {
			"" => Self::default(),
			"mtls" | "mtls-certificate" | "nginx" => Self::Mtls,
			"xfcc" | "x-forwarded-client-cert" | "envoy" => Self::Xfcc,
			other => {
				// Never guess on a security switch: an unrecognised value
				// keeps the live path rather than silently trusting another.
				warn!(
					"{CERT_HEADER_ENV}: unrecognised value {other:?}, keeping the default \
					 ({:?}). Valid values are \"mtls\" and \"xfcc\".",
					Self::default()
				);
				Self::default()
			}
		}
	}
}

/// Resolve a request to a [`Device`] via mTLS. Returns:
///
/// - `Ok(Some(device))` — cert present and its key matches a known device.
/// - `Ok(None)` — no cert header at all (caller may try another path).
/// - `Err(AuthCertificateNotFound)` — cert present but its key is unknown.
/// - `Err(_)` — header present but malformed.
pub async fn resolve(
	parts: &Parts,
	db: &mut AsyncPgConnection,
	trusted: ClientCertHeader,
) -> Result<Option<Device>> {
	let Some(key) = spki_from_headers(&parts.headers, trusted)? else {
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
pub fn spki_from_headers(
	headers: &http::HeaderMap,
	trusted: ClientCertHeader,
) -> Result<Option<Vec<u8>>> {
	let Some(pem) = extract_cert_pem_with(headers, trusted)? else {
		return Ok(None);
	};

	let (_, der) = parse_x509_pem(pem.as_bytes())
		.map_err(|e| AppError::AuthInvalidCertificate(format!("Invalid PEM format: {}", e)))?;
	let (_, cert) = parse_x509_certificate(&der.contents).map_err(|e| {
		AppError::AuthInvalidCertificate(format!("Invalid X.509 certificate: {}", e))
	})?;

	Ok(Some(cert.tbs_certificate.subject_pki.raw.to_vec()))
}

/// [`extract_cert_pem`] against an explicit trust configuration, so the
/// gating is testable without touching process-wide env.
fn extract_cert_pem_with(
	headers: &http::HeaderMap,
	trusted: ClientCertHeader,
) -> Result<Option<String>> {
	// Only the header the configured ingress sets is read. Any other is
	// client-supplied as far as this server can tell.
	if trusted == ClientCertHeader::Xfcc {
		let Some(cert_value) = headers
			.get("x-forwarded-client-cert")
			.and_then(|v| v.to_str().ok())
			.and_then(xfcc_client_cert)
		else {
			return Ok(None);
		};
		return Ok(Some(
			percent_encoding::percent_decode(cert_value.as_bytes())
				.decode_utf8()
				.map_err(|e| {
					AppError::AuthInvalidCertificate(format!("Invalid UTF-8 in certificate: {}", e))
				})?
				.into_owned(),
		));
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

	fn extracted(pairs: &[(&str, &str)], trusted: ClientCertHeader) -> Option<String> {
		extract_cert_pem_with(&headers(pairs), trusted).expect("well-formed headers")
	}

	/// The shipped default is the live ingress path.
	#[test]
	fn the_default_is_the_nginx_header() {
		assert_eq!(ClientCertHeader::default(), ClientCertHeader::Mtls);
	}

	/// The bug the gate exists for. Envoy is not deployed, and nginx has no
	/// reason to strip a header it doesn't use, so an untrusted caller could
	/// set XFCC itself — and it was *preferred* over the header nginx
	/// actually verifies.
	#[test]
	fn xfcc_is_ignored_on_the_nginx_path() {
		assert_eq!(
			extracted(
				&[("x-forwarded-client-cert", &format!("Cert={PEM}"))],
				ClientCertHeader::Mtls,
			),
			None,
			"an untrusted XFCC header must not authenticate anything",
		);
	}

	/// And it must not be able to override the genuinely-verified one.
	#[test]
	fn xfcc_cannot_override_the_nginx_header() {
		let nginx = "-----BEGIN%20CERTIFICATE-----NGINX";
		assert_eq!(
			extracted(
				&[
					("x-forwarded-client-cert", &format!("Cert={PEM}")),
					("mtls-certificate", nginx),
				],
				ClientCertHeader::Mtls,
			)
			.as_deref(),
			Some("-----BEGIN CERTIFICATE-----NGINX"),
		);
	}

	/// The mirror image: on the Envoy path the nginx headers are the
	/// client-supplied ones, and must not be read either.
	#[test]
	fn the_nginx_headers_are_ignored_on_the_envoy_path() {
		for header in ["mtls-certificate", "ssl-client-cert"] {
			assert_eq!(
				extracted(&[(header, PEM)], ClientCertHeader::Xfcc),
				None,
				"{header} must not be read on the Envoy path",
			);
		}
	}

	#[test]
	fn xfcc_is_honoured_on_the_envoy_path() {
		assert_eq!(
			extracted(
				&[("x-forwarded-client-cert", &format!("Cert={PEM}"))],
				ClientCertHeader::Xfcc,
			)
			.as_deref(),
			Some("-----BEGIN CERTIFICATE-----A"),
		);
	}

	#[test]
	fn env_parses_both_spellings_and_keeps_the_default_otherwise() {
		let key = CERT_HEADER_ENV;
		for (raw, expected) in [
			("mtls", ClientCertHeader::Mtls),
			("nginx", ClientCertHeader::Mtls),
			(" MTLS ", ClientCertHeader::Mtls),
			("xfcc", ClientCertHeader::Xfcc),
			("envoy", ClientCertHeader::Xfcc),
			("X-Forwarded-Client-Cert", ClientCertHeader::Xfcc),
		] {
			// SAFETY: single-threaded test; the key is only read here.
			unsafe { std::env::set_var(key, raw) };
			assert_eq!(ClientCertHeader::from_env(), expected, "{raw:?}");
		}
		// Empty or unrecognised keeps the live path rather than guessing.
		for raw in ["", "   ", "both", "none", "true"] {
			unsafe { std::env::set_var(key, raw) };
			assert_eq!(
				ClientCertHeader::from_env(),
				ClientCertHeader::Mtls,
				"{raw:?} must not select a non-default header",
			);
		}
		unsafe { std::env::remove_var(key) };
		assert_eq!(ClientCertHeader::from_env(), ClientCertHeader::Mtls);
	}
}
