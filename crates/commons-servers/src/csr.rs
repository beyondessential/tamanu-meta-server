//! Validating a certificate signing request a server has submitted.
//!
//! Canopy certifies exactly the name a server asked for and nothing else, so
//! the request is checked against that name rather than trusted or trimmed: a
//! CSR carrying a second name would otherwise let one server obtain a
//! certificate valid for another group, and one carrying fewer would leave
//! the server serving something it did not expect.
// spec: CRT#requesting

use commons_errors::{AppError, Result};
use commons_types::dns::normalize_domain;
use ring::digest::{SHA256, digest};
use x509_parser::prelude::*;
use x509_parser::public_key::PublicKey;

/// The shortest RSA modulus Canopy will certify. Stated here rather than left to
/// whatever the authority happens to accept this year.
const MIN_RSA_BITS: usize = 2048;

/// A signing request that has been parsed, had its signature checked, and been
/// confirmed to ask for exactly one expected name.
#[derive(Debug, Clone)]
pub struct ValidatedCsr {
	/// The single name the request is for, normalised.
	pub name: String,
	/// The request as submitted, to be handed to the authority verbatim.
	pub der: Vec<u8>,
	/// Hex SHA-256 over the subject public key info — the identity of the key
	/// being certified. Canopy holds this so a repeat request for the same key
	/// can be answered from the certificate it already has, while a request for
	/// a different key is recognised as needing a new one.
	pub key_fingerprint: String,
}

/// Parse and check a submitted CSR against the name it is supposed to be for.
///
/// Rejects a request whose signature doesn't verify (so the sender demonstrably
/// holds the key), whose names are anything other than exactly `requested_name`,
/// or whose key is too weak to be worth certifying.
pub fn validate_csr(der: &[u8], requested_name: &str) -> Result<ValidatedCsr> {
	let expected = normalize_domain(requested_name)?;

	let (rest, csr) = X509CertificationRequest::from_der(der)
		.map_err(|e| AppError::BadRequest(format!("could not parse the signing request: {e}")))?;
	if !rest.is_empty() {
		return Err(AppError::BadRequest(
			"the signing request has trailing bytes".into(),
		));
	}

	// Proof of possession: whoever sent this holds the private half.
	csr.verify_signature().map_err(|e| {
		AppError::BadRequest(format!("the signing request's signature is not valid: {e}"))
	})?;

	let info = &csr.certification_request_info;

	// Every name the request asks to have certified, from the subject common
	// name and from the requested SAN extension. Both are collected because
	// either can carry a name, and a name in one that isn't in the other is
	// still a name the certificate would bear.
	let mut asked: Vec<String> = Vec::new();
	for cn in info.subject.iter_common_name() {
		let raw = cn.as_str().map_err(|e| {
			AppError::BadRequest(format!("the subject common name is not readable text: {e}"))
		})?;
		asked.push(raw.to_string());
	}
	if let Some(extensions) = csr.requested_extensions() {
		for extension in extensions {
			if let ParsedExtension::SubjectAlternativeName(san) = extension {
				for name in &san.general_names {
					match name {
						GeneralName::DNSName(dns) => asked.push((*dns).to_string()),
						other => {
							return Err(AppError::BadRequest(format!(
								"the signing request asks for a non-DNS name ({other}); Canopy \
								 certifies DNS names only"
							)));
						}
					}
				}
			}
		}
	}

	if asked.is_empty() {
		return Err(AppError::BadRequest(format!(
			"the signing request names nothing to certify; it must ask for {expected} and nothing \
			 else"
		)));
	}

	// Normalise before comparing so a trailing dot or different case isn't
	// mistaken for a different name. A wildcard fails normalisation, which is
	// how wildcards are refused.
	let mut normalised: Vec<String> = Vec::with_capacity(asked.len());
	for name in &asked {
		let one = normalize_domain(name).map_err(|e| {
			AppError::BadRequest(format!(
				"the signing request asks for {name:?}, which is not a \
				 name Canopy can certify: {e}"
			))
		})?;
		if !normalised.contains(&one) {
			normalised.push(one);
		}
	}

	if normalised.len() != 1 || normalised[0] != expected {
		return Err(AppError::BadRequest(format!(
			"the signing request asks for {} but the request is for {expected}; Canopy certifies \
			 exactly the name requested",
			normalised.join(", "),
		)));
	}

	check_key_strength(&info.subject_pki)?;

	Ok(ValidatedCsr {
		name: expected,
		der: der.to_vec(),
		key_fingerprint: hex_sha256(info.subject_pki.raw),
	})
}

/// Refuse a key not worth certifying: an under-sized RSA modulus, or an
/// algorithm Canopy doesn't recognise well enough to judge.
fn check_key_strength(spki: &SubjectPublicKeyInfo<'_>) -> Result<()> {
	match spki.parsed() {
		Ok(PublicKey::RSA(rsa)) => {
			let bits = rsa.key_size();
			if bits < MIN_RSA_BITS {
				return Err(AppError::BadRequest(format!(
					"the key is a {bits}-bit RSA key; Canopy certifies RSA keys of at least \
					 {MIN_RSA_BITS} bits"
				)));
			}
			Ok(())
		}
		// Any curve the parser recognises is one a public authority will accept.
		Ok(PublicKey::EC(_)) => Ok(()),
		Ok(other) => Err(AppError::BadRequest(format!(
			"the key is of a kind Canopy does not certify ({other:?}); use an elliptic-curve or \
			 RSA key"
		))),
		Err(e) => Err(AppError::BadRequest(format!(
			"the key in the signing request could not be read: {e}"
		))),
	}
}

fn hex_sha256(bytes: &[u8]) -> String {
	let mut out = String::with_capacity(64);
	for byte in digest(&SHA256, bytes).as_ref() {
		out.push_str(&format!("{byte:02x}"));
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};

	/// A CSR for `names`, with the first also set as the subject common name —
	/// the shape a normal client produces.
	fn csr_for(names: &[&str]) -> (Vec<u8>, KeyPair) {
		let key = KeyPair::generate().expect("generate key");
		let mut params =
			CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>())
				.expect("params");
		let mut dn = DistinguishedName::new();
		dn.push(DnType::CommonName, names[0]);
		params.distinguished_name = dn;
		let csr = params.serialize_request(&key).expect("serialize csr");
		(csr.der().to_vec(), key)
	}

	#[test]
	fn accepts_a_request_for_exactly_the_name() {
		let (der, _key) = csr_for(&["central.fiji.tamanu.app"]);
		let validated = validate_csr(&der, "central.fiji.tamanu.app").expect("valid");
		assert_eq!(validated.name, "central.fiji.tamanu.app");
		assert_eq!(validated.key_fingerprint.len(), 64);
		assert_eq!(validated.der, der);
	}

	#[test]
	fn normalises_the_requested_name() {
		let (der, _key) = csr_for(&["central.fiji.tamanu.app"]);
		validate_csr(&der, "Central.Fiji.Tamanu.App.").expect("case and dot are not significant");
	}

	#[test]
	fn refuses_a_smuggled_second_name() {
		let (der, _key) = csr_for(&["central.fiji.tamanu.app", "central.samoa.tamanu.app"]);
		let err = validate_csr(&der, "central.fiji.tamanu.app")
			.expect_err("a second name must not ride along");
		assert!(matches!(err, AppError::BadRequest(_)), "got {err:?}");
	}

	#[test]
	fn refuses_a_request_for_another_name() {
		let (der, _key) = csr_for(&["central.samoa.tamanu.app"]);
		validate_csr(&der, "central.fiji.tamanu.app")
			.expect_err("the CSR must be for the requested name");
	}

	#[test]
	fn refuses_a_wildcard() {
		let (der, _key) = csr_for(&["*.fiji.tamanu.app"]);
		validate_csr(&der, "*.fiji.tamanu.app").expect_err("wildcards are not certified");
	}

	#[test]
	fn refuses_junk() {
		validate_csr(b"not a csr at all", "central.fiji.tamanu.app").expect_err("unparseable");
	}

	#[test]
	fn refuses_trailing_bytes() {
		let (mut der, _key) = csr_for(&["central.fiji.tamanu.app"]);
		der.extend_from_slice(b"extra");
		validate_csr(&der, "central.fiji.tamanu.app").expect_err("trailing bytes");
	}

	#[test]
	fn the_fingerprint_follows_the_key() {
		let (a, _) = csr_for(&["central.fiji.tamanu.app"]);
		let (b, _) = csr_for(&["central.fiji.tamanu.app"]);
		let one = validate_csr(&a, "central.fiji.tamanu.app").expect("valid");
		let two = validate_csr(&b, "central.fiji.tamanu.app").expect("valid");
		assert_ne!(
			one.key_fingerprint, two.key_fingerprint,
			"a different key must be a different fingerprint"
		);

		let again = validate_csr(&a, "central.fiji.tamanu.app").expect("valid");
		assert_eq!(one.key_fingerprint, again.key_fingerprint);
	}
}
