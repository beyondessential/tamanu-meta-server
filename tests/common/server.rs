use ::time::OffsetDateTime;
use axum::extract::connect_info::MockConnectInfo;
use axum_client_ip::ClientIpSource;
use axum_test::TestServer;
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use percent_encoding::utf8_percent_encode;
use rcgen::{
	CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
	PKCS_ECDSA_P256_SHA256,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tamanu_meta::{private_routes, public_routes, router, state::AppState};
use uuid::Uuid;
use x509_parser::prelude::*;

#[derive(QueryableByName)]
struct Device {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

fn make_certificate() -> rcgen::Certificate {
	let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("keygen");
	let mut cert = CertificateParams::default();
	cert.is_ca = IsCa::NoCa;
	cert.not_before = OffsetDateTime::now_utc();
	cert.key_usages = vec![KeyUsagePurpose::DigitalSignature];
	cert.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
	cert.use_authority_key_identifier_extension = true;
	cert.distinguished_name = DistinguishedName::new();
	cert.self_signed(&key).expect("sign cert")
}

#[path = "./db.rs"]
mod test_db;

pub async fn run<F, T, Fut>(test: F) -> T
where
	F: FnOnce(AsyncPgConnection, TestServer, TestServer) -> Fut,
	Fut: Future<Output = T>,
{
	test_db::TestDb::run(async |conn, url| {
		let state = AppState {
			db: AppState::init_db_to(&url),
			tera: AppState::init_tera().unwrap(),
		};

		// Add ConnectInfo layer for test servers
		let mock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
		let public_router = router(state.clone(), public_routes(), ClientIpSource::ConnectInfo)
			.layer(MockConnectInfo(mock_addr));
		let private_router = router(
			state.clone(),
			private_routes("/$".into()),
			ClientIpSource::ConnectInfo,
		)
		.layer(MockConnectInfo(mock_addr));

		test(
			conn,
			TestServer::new(public_router).unwrap(),
			TestServer::new(private_router).unwrap(),
		)
		.await
	})
	.await
}

#[allow(dead_code)] // when imported into a test that only uses run()
pub async fn run_with_device_auth<F, T, Fut>(role: &'static str, test: F) -> T
where
	F: FnOnce(AsyncPgConnection, String, Uuid, TestServer, TestServer) -> Fut,
	Fut: Future<Output = T>,
{
	run(async |mut conn, public, private| {
		let cert = make_certificate();
		let cert_pem = cert.pem();
		let cert = utf8_percent_encode(&cert_pem, &percent_encoding::NON_ALPHANUMERIC).to_string();

		let (_, pem_parsed) = parse_x509_pem(cert_pem.as_bytes()).expect("parse pem");
		let (_, x509_cert) = parse_x509_certificate(&pem_parsed.contents).expect("parse cert");
		let key_data = x509_cert.tbs_certificate.subject_pki.raw.to_vec();

		let device_row: Device = sql_query(
			r#"
				INSERT INTO devices (key_data, role)
				VALUES ($1, $2::device_role)
				RETURNING id
			"#,
		)
		.bind::<sql_types::Binary, _>(key_data)
		.bind::<sql_types::Text, _>(role)
		.get_result(&mut conn)
		.await
		.expect("insert device");
		let device_id = device_row.id;

		test(conn, cert, device_id, public, private).await
	})
	.await
}
