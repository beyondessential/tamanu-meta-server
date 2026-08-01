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
use x509_parser::prelude::*;

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
	// Prefer x-forwarded-client-cert (Envoy XFCC format) when present,
	// falling back to mtls-certificate and ssl-client-cert headers.
	let xfcc_cert = headers
		.get("x-forwarded-client-cert")
		.and_then(|v| v.to_str().ok())
		.and_then(xfcc_client_cert);

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
}
