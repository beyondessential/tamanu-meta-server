use rcgen::{CertificateParams, ExtendedKeyUsagePurpose, KeyPair, KeyUsagePurpose, PKCS_ED25519};
use time::OffsetDateTime;

fn main() {
	let key = KeyPair::generate_for(&PKCS_ED25519).expect("keygen");
	let mut cert = CertificateParams::default();
	cert.not_before = OffsetDateTime::now_utc();
	cert.key_usages = vec![KeyUsagePurpose::DigitalSignature];
	cert.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
	cert.use_authority_key_identifier_extension = true;
	let cert = cert.self_signed(&key).expect("sign cert");

	std::fs::write("identity.crt", cert.pem()).expect("write identity.crt");
	std::fs::write("identity.key", key.serialize_pem()).expect("write identity.key");
}
