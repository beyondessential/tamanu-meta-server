use ::time::OffsetDateTime;
use axum_client_ip::ClientIpSource;
use axum_test::TestServer;
use commons_servers::router;
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use percent_encoding::utf8_percent_encode;
use rcgen::{
	CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
	PKCS_ECDSA_P256_SHA256,
};
use uuid::Uuid;
use x509_parser::prelude::*;

use crate::db::TestDb;

#[derive(QueryableByName)]
struct Device {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

pub fn make_certificate() -> (Vec<u8>, String) {
	let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("keygen");
	let mut cert = CertificateParams::default();
	cert.is_ca = IsCa::NoCa;
	cert.not_before = OffsetDateTime::now_utc();
	cert.key_usages = vec![KeyUsagePurpose::DigitalSignature];
	cert.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
	cert.use_authority_key_identifier_extension = true;
	cert.distinguished_name = DistinguishedName::new();
	let cert = cert.self_signed(&key).expect("sign cert");

	let cert_pem = cert.pem();
	let cert = utf8_percent_encode(&cert_pem, percent_encoding::NON_ALPHANUMERIC).to_string();

	let (_, pem_parsed) = parse_x509_pem(cert_pem.as_bytes()).expect("parse pem");
	let (_, x509_cert) = parse_x509_certificate(&pem_parsed.contents).expect("parse cert");
	let key_data = x509_cert.tbs_certificate.subject_pki.raw.to_vec();

	(key_data, cert)
}

pub async fn run<F, T, Fut>(test: F) -> T
where
	F: FnOnce(AsyncPgConnection, TestServer, TestServer) -> Fut,
	Fut: Future<Output = T>,
{
	TestDb::run(async |conn, url| {
		let public_router = router(
			public_server::routes().with_state(public_server::state::AppState {
				db: database::init_to(&url),
				tera: public_server::state::AppState::init_tera().unwrap(),
			}),
			ClientIpSource::RightmostForwarded,
		);
		let private_router = router(
			private_server::routes(private_server::state::AppState::from_db_url(&url).unwrap())
				.unwrap(),
			ClientIpSource::RightmostForwarded,
		);

		let mut public_server = TestServer::new(public_router).unwrap();
		public_server.add_header("Forwarded", "for=192.0.1.60");

		let mut private_server = TestServer::new(private_router).unwrap();
		private_server.add_header("Forwarded", "for=192.0.2.60");

		test(conn, public_server, private_server).await
	})
	.await
}

#[allow(dead_code)] // when imported into a test that only uses run()
pub async fn run_with_device_auth<F, T, Fut>(role: &'static str, test: F) -> T
where
	F: FnOnce(AsyncPgConnection, String, Uuid, TestServer, TestServer) -> Fut,
	Fut: Future<Output = T>,
{
	run(async |mut conn, mut public, private| {
		let (key_data, cert) = make_certificate();

		let device_row: Device = sql_query(
			r#"
				INSERT INTO devices (role)
				VALUES ($1)
				RETURNING id
			"#,
		)
		.bind::<sql_types::Text, _>(role)
		.get_result(&mut conn)
		.await
		.expect("insert device");
		let device_id = device_row.id;

		sql_query(
			r#"
				INSERT INTO device_keys (device_id, key_data, name, is_active)
				VALUES ($1, $2, 'Test Key', true)
			"#,
		)
		.bind::<sql_types::Uuid, _>(device_id)
		.bind::<sql_types::Binary, _>(key_data)
		.execute(&mut conn)
		.await
		.expect("insert device key");

		public.add_header("X-Version", "3.4.5");
		test(conn, cert, device_id, public, private).await
	})
	.await
}
