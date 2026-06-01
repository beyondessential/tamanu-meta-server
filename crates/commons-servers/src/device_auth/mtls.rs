//! mTLS path of the device-auth extractor. Reads a client certificate
//! from one of the supported headers, derives a stable public-key blob,
//! and resolves it to an existing trusted [`Device`] row.
//!
//! First-contact auto-creation was deliberately removed: a device row is
//! born only through the gated enrollment flow (`/servers/register/*`), so
//! merely connecting once from the internet can no longer mint an `Untrusted`
//! device. An unknown key here is `AuthCertificateNotFound`.

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
		.and_then(|v| {
			// XFCC format: comma-separated elements, each with semicolon-separated fields
			v.split(',')
				.next()
				.unwrap_or("")
				.split(';')
				.find_map(|field| field.strip_prefix("Cert="))
		});

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
