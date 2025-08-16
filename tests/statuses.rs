use ::time::OffsetDateTime;
use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use percent_encoding::utf8_percent_encode;
use rcgen::{
	CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
	PKCS_ECDSA_P256_SHA256,
};
use uuid::Uuid;
use x509_parser::prelude::*;

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

#[path = "common/server.rs"]
mod test_server;

#[derive(QueryableByName)]
struct StatusResult {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
	#[diesel(sql_type = sql_types::Uuid)]
	server_id: Uuid,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
	device_id: Option<Uuid>,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Text>)]
	version: Option<String>,
	#[diesel(sql_type = sql_types::Jsonb)]
	extra: serde_json::Value,
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status() {
	test_server::run(async |mut conn, public, _| {
		// 1. Generate a certificate and extract the public key
		let cert = make_certificate();
		let cert_pem = cert.pem();

		// Parse the certificate to get the public key data for device registration
		let (_remainder, pem_parsed) = parse_x509_pem(cert_pem.as_bytes()).expect("parse pem");
		let (_remainder, x509_cert) =
			parse_x509_certificate(&pem_parsed.contents).expect("parse cert");
		let key_data = x509_cert.tbs_certificate.subject_pki.raw.to_vec();

		// 2. Insert a device with server role
		let device_row: StatusResult = sql_query(
			r#"
				INSERT INTO devices (key_data, role)
				VALUES ($1, 'server')
				RETURNING id, id as server_id, id as device_id, 'test' as version, '{}'::jsonb as extra
			"#,
		)
		.bind::<sql_types::Binary, _>(key_data)
		.get_result(&mut conn)
		.await
		.expect("insert device");
		let device_id = device_row.id;

		// 3. Insert a server and associate it with the device
		let server_id = Uuid::new_v4();
		sql_query(
			r#"
				INSERT INTO servers (id, host, kind, device_id)
				VALUES ($1, 'https://test.example.com', 'facility', $2)
			"#,
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
		.execute(&mut conn)
		.await
		.expect("insert server");

		// 4. URL encode the certificate for the header
		let encoded_cert =
			utf8_percent_encode(&cert_pem, &percent_encoding::NON_ALPHANUMERIC).to_string();

		// 5. Submit the status with proper authentication
		let status_data = serde_json::json!({
			"uptime": 3600,
			"memory_usage": 0.75,
			"disk_space": 0.85
		});

		let response = public
			.post(&format!("/status/{}", server_id))
			.add_header("mtls-certificate", &encoded_cert)
			.add_header("X-Version", "1.2.3")
			.json(&status_data)
			.await;

		// 6. Verify the response is successful
		response.assert_status_ok();
		response.assert_header("content-type", "application/json");

		// 7. Verify the returned status data
		let returned_status: serde_json::Value = response.json();

		assert!(returned_status.get("id").is_some());
		assert_eq!(
			returned_status.get("server_id").and_then(|v| v.as_str()),
			Some(server_id.to_string().as_str())
		);
		assert_eq!(
			returned_status.get("device_id").and_then(|v| v.as_str()),
			Some(device_id.to_string().as_str())
		);
		assert_eq!(
			returned_status.get("version").and_then(|v| v.as_str()),
			Some("1.2.3")
		);

		// Verify extra data was stored
		let extra = returned_status.get("extra").expect("extra field");
		assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(3600));
		assert_eq!(
			extra.get("memory_usage").and_then(|v| v.as_f64()),
			Some(0.75)
		);
		assert_eq!(extra.get("disk_space").and_then(|v| v.as_f64()), Some(0.85));

		// 8. Verify the status was actually stored in the database
		let db_status: StatusResult = sql_query(
			r#"
				SELECT id, server_id, device_id, version, extra
				FROM statuses
				WHERE server_id = $1
				ORDER BY created_at DESC
				LIMIT 1
			"#,
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.get_result(&mut conn)
		.await
		.expect("fetch created status");

		assert_eq!(db_status.server_id, server_id);
		assert_eq!(db_status.device_id, Some(device_id));
		assert_eq!(db_status.version.as_deref(), Some("1.2.3"));
		assert_eq!(
			db_status.extra.get("uptime").and_then(|v| v.as_i64()),
			Some(3600)
		);
	})
	.await
}
