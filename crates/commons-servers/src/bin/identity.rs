use rcgen::{
	CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
	PKCS_ECDSA_P256_SHA256,
};
use time::OffsetDateTime;

fn main() {
	let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("keygen");
	let mut cert = CertificateParams::default();
	cert.is_ca = IsCa::NoCa;
	cert.not_before = OffsetDateTime::now_utc();
	cert.key_usages = vec![KeyUsagePurpose::DigitalSignature];
	cert.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
	cert.use_authority_key_identifier_extension = true;
	cert.distinguished_name = DistinguishedName::new();
	let cert = cert.self_signed(&key).expect("sign cert");

	std::fs::write("identity.crt.pem", cert.pem()).expect("write identity.crt.pem");
	std::fs::write("identity.key.pem", key.serialize_pem()).expect("write identity.key.pem");
	std::fs::write("identity.pub.pem", key.public_key_pem()).expect("write identity.pub.pem");
}
