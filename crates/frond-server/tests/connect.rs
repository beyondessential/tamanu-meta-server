use std::{
	net::{Ipv4Addr, SocketAddr, SocketAddrV4},
	sync::Arc,
	time::Duration,
};

use ed25519_dalek::pkcs8::EncodePrivateKey;
use frond_server::{ALPN, keys};
use rustls::{
	ClientConfig as TlsClientConfig, DigitallySignedStruct, SignatureScheme,
	client::{
		AlwaysResolvesClientRawPublicKeys,
		danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
	},
	sign::CertifiedKey,
};
use rustls_pki_types::{
	CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, SubjectPublicKeyInfoDer,
	UnixTime,
};

fn install_provider() {
	use std::sync::Once;
	static ONCE: Once = Once::new();
	ONCE.call_once(|| {
		rustls::crypto::ring::default_provider()
			.install_default()
			.expect("install ring crypto provider");
	});
}

#[derive(Debug)]
struct AcceptAnyServer;

impl ServerCertVerifier for AcceptAnyServer {
	fn verify_server_cert(
		&self,
		_end_entity: &CertificateDer<'_>,
		_intermediates: &[CertificateDer<'_>],
		_server_name: &ServerName<'_>,
		_ocsp: &[u8],
		_now: UnixTime,
	) -> Result<ServerCertVerified, rustls::Error> {
		Ok(ServerCertVerified::assertion())
	}

	fn verify_tls12_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		rustls::crypto::verify_tls12_signature(
			message,
			cert,
			dss,
			&rustls::crypto::ring::default_provider().signature_verification_algorithms,
		)
	}

	fn verify_tls13_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		rustls::crypto::verify_tls13_signature_with_raw_key(
			message,
			&SubjectPublicKeyInfoDer::from(cert.as_ref()),
			dss,
			&rustls::crypto::ring::default_provider().signature_verification_algorithms,
		)
	}

	fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
		rustls::crypto::ring::default_provider()
			.signature_verification_algorithms
			.supported_schemes()
	}

	fn requires_raw_public_keys(&self) -> bool {
		true
	}
}

fn build_test_client_config() -> TlsClientConfig {
	let key = keys::generate_ephemeral();
	let spki = keys::spki_der(&key);
	let pkcs8 = key.to_pkcs8_der().unwrap();
	let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8.as_bytes().to_vec()));
	let signing = rustls::crypto::ring::sign::any_supported_type(&private_key).unwrap();
	let cert = CertificateDer::from(spki);
	let certified = Arc::new(CertifiedKey::new(vec![cert], signing));
	let resolver = Arc::new(AlwaysResolvesClientRawPublicKeys::new(certified));

	let mut tls = TlsClientConfig::builder()
		.dangerous()
		.with_custom_certificate_verifier(Arc::new(AcceptAnyServer))
		.with_client_cert_resolver(resolver);
	tls.alpn_protocols = vec![ALPN.to_vec()];
	tls
}

#[tokio::test(flavor = "multi_thread")]
async fn handshake_negotiates_bes_canopy_1() {
	install_provider();

	let bind_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
	let server_endpoint = frond_server::bind(bind_addr).expect("bind server");
	let server_addr = server_endpoint.local_addr().expect("local addr");
	tokio::spawn(frond_server::accept_loop(server_endpoint));

	let tls = build_test_client_config();
	let quic = quinn::crypto::rustls::QuicClientConfig::try_from(tls).expect("quic config");
	let client_cfg = quinn::ClientConfig::new(Arc::new(quic));
	let mut client = quinn::Endpoint::client(SocketAddr::V4(SocketAddrV4::new(
		Ipv4Addr::UNSPECIFIED,
		0,
	)))
	.expect("client endpoint");
	client.set_default_client_config(client_cfg);

	let conn = tokio::time::timeout(
		Duration::from_secs(5),
		client.connect(server_addr, "frond").expect("connect call"),
	)
	.await
	.expect("connect timeout")
	.expect("handshake");

	let handshake = conn.handshake_data().expect("handshake data");
	let data = handshake
		.downcast::<quinn::crypto::rustls::HandshakeData>()
		.expect("rustls handshake data");
	assert_eq!(
		data.protocol.as_deref(),
		Some(ALPN),
		"server should negotiate the bes.canopy/1 ALPN"
	);

	conn.close(0u32.into(), b"test done");
	client.wait_idle().await;
}
