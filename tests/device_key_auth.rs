use diesel_async::SimpleAsyncConnection;

use crate::test_server::make_certificate;

#[path = "common/server.rs"]
mod test_server;

// Tests to verify that device key authentication works with the new split schema

#[tokio::test(flavor = "multi_thread")]
async fn device_key_authentication_works() {
	test_server::run_with_device_auth("releaser", async |mut conn, cert, _device_id, public, _| {
		// Create a version first so we can test authenticated endpoint
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (1, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		// Test that the device authentication works with the new schema
		let response = public
			.post("/versions/1.0.1")
			.add_header("mtls-certificate", &cert)
			.text("changelog for 1.0.1")
			.await;

		response.assert_status_ok();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_keys_per_device() {
	test_server::run_with_device_auth("server", async |mut conn, _cert, device_id, _public, _| {
		// The run_with_device_auth helper already created one key, let's add another
		use tamanu_meta::db::devices::DeviceKey;

		let additional_key_data = b"additional-server-key-data";
		let _additional_key = DeviceKey::create(
			&mut conn,
			device_id,
			additional_key_data.to_vec(),
			Some("Additional Server Key".to_string()),
		)
		.await
		.unwrap();

		// Verify both keys can authenticate as the same device

		// Test the original key (from cert)
		let keys = DeviceKey::find_by_device(&mut conn, device_id)
			.await
			.unwrap();
		assert_eq!(keys.len(), 2); // Initial key + additional key

		// All keys should belong to the same device
		for key in &keys {
			assert_eq!(key.device_id, device_id);
			assert!(key.is_active);
		}
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn inactive_keys_not_used_for_auth() {
	test_server::run_with_device_auth("admin", async |mut conn, cert, device_id, public, _| {
		// First verify the key works
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (2, 0, 0, 'Test version', true)",
		)
		.await
		.unwrap();

		let response_before = public
			.post("/versions/2.0.1")
			.add_header("mtls-certificate", &cert)
			.text("changelog before deactivation")
			.await;
		response_before.assert_status_ok();

		// Deactivate all keys for this device
		use tamanu_meta::db::devices::DeviceKey;
		let keys = DeviceKey::find_by_device(&mut conn, device_id).await.unwrap();
		for key in keys {
			DeviceKey::deactivate(&mut conn, key.id).await.unwrap();
		}

		// Now authentication should fail
		let response_after = public
			.post("/versions/2.0.2")
			.add_header("mtls-certificate", &cert)
			.text("changelog after deactivation")
			.await;
		response_after.assert_status_not_ok();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn device_without_keys_cannot_auth() {
	test_server::run(async |mut conn, public, _| {
		// Create a device without any keys (not using run_with_device_auth)
		conn.batch_execute(
			"INSERT INTO devices (id, role) VALUES ('44444444-4444-4444-4444-444444444444', 'admin')",
		)
		.await
		.unwrap();

		// Try to authenticate - should fail since device has no keys
		let response = public.post("/versions/1.0.1").text("changelog").await;

		response.assert_status_not_ok();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn device_key_creation_via_api() {
	test_server::run(async |mut conn, _public, _| {
		// This test verifies that our Device::create method works with the new schema
		// We can't test the actual HTTP authentication, but we can test the database logic

		// Create a test key
		let key_data = b"new-device-key-12345";

		// This would normally be called by the authentication middleware
		// when a new device connects for the first time
		use tamanu_meta::db::devices::Device;

		let device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();

		// Verify the device was created
		assert!(!device.id.to_string().is_empty());

		// Verify the key was created by checking that from_key can find it
		let found_device = Device::from_key(&mut conn, key_data).await.unwrap();
		assert!(found_device.is_some());
		assert_eq!(found_device.unwrap().id, device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_keys_authenticate_same_device() {
	test_server::run_with_device_auth("admin", async |mut conn, cert, device_id, public, _| {
		// Create additional keys for the same device
		use tamanu_meta::db::devices::DeviceKey;

		let backup_key_data = b"backup-admin-key-0987654321";
		let emergency_key_data = b"emergency-admin-key-abcdef";

		let _backup_key = DeviceKey::create(
			&mut conn,
			device_id,
			backup_key_data.to_vec(),
			Some("Backup Admin Key".to_string()),
		)
		.await
		.unwrap();

		let _emergency_key = DeviceKey::create(
			&mut conn,
			device_id,
			emergency_key_data.to_vec(),
			Some("Emergency Admin Key".to_string()),
		)
		.await
		.unwrap();

		// Verify all keys authenticate as the same device
		use tamanu_meta::db::devices::Device;

		let device_via_backup = Device::from_key(&mut conn, backup_key_data).await.unwrap();
		let device_via_emergency = Device::from_key(&mut conn, emergency_key_data).await.unwrap();

		assert!(device_via_backup.is_some());
		assert!(device_via_emergency.is_some());

		let device_backup = device_via_backup.unwrap();
		let device_emergency = device_via_emergency.unwrap();

		// All should be the same device
		assert_eq!(device_backup.id, device_id);
		assert_eq!(device_emergency.id, device_id);

		// All should have the same role
		use tamanu_meta::db::device_role::DeviceRole;
		assert_eq!(device_backup.role, DeviceRole::Admin);
		assert_eq!(device_emergency.role, DeviceRole::Admin);

		// Test actual HTTP authentication with the certificate
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (3, 0, 0, 'Multi-key test', true)",
		)
		.await
		.unwrap();

		let response = public
			.post("/versions/3.0.1")
			.add_header("mtls-certificate", &cert)
			.text("changelog from admin device")
			.await;
		response.assert_status_ok();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn key_deactivation_works() {
	test_server::run_with_device_auth("server", async |mut conn, cert, device_id, public, _| {
		// Create a test server and status endpoint
		use diesel::{sql_query, sql_types};
		use diesel_async::RunQueryDsl;
		let server_id = uuid::Uuid::parse_str("77777777-7777-7777-7777-777777777777").unwrap();
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

		// Verify authentication works initially
		let response_before = public
			.post("/status/77777777-7777-7777-7777-777777777777")
			.add_header("mtls-certificate", &cert)
			.json(&serde_json::json!({"uptime": 3600}))
			.await;
		response_before.assert_status_ok();

		// Find and deactivate the device's key
		use tamanu_meta::db::devices::DeviceKey;
		let keys = DeviceKey::find_by_device(&mut conn, device_id)
			.await
			.unwrap();
		assert!(!keys.is_empty());

		DeviceKey::deactivate(&mut conn, keys[0].id).await.unwrap();

		// Verify authentication now fails
		let response_after = public
			.post("/status/77777777-7777-7777-7777-777777777777")
			.add_header("mtls-certificate", &cert)
			.json(&serde_json::json!({"uptime": 7200}))
			.await;
		response_after.assert_status_not_ok();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn device_key_management() {
	test_server::run_with_device_auth("releaser", async |mut conn, cert, device_id, public, _| {
		// Create additional keys for the device (run_with_device_auth already created one)
		use tamanu_meta::db::devices::DeviceKey;

		let key2_data = b"releaser-key-secondary";
		let key3_data = b"releaser-key-tertiary";

		let _key2 = DeviceKey::create(
			&mut conn,
			device_id,
			key2_data.to_vec(),
			Some("Secondary Releaser Key".to_string()),
		)
		.await
		.unwrap();

		let _key3 = DeviceKey::create(
			&mut conn,
			device_id,
			key3_data.to_vec(),
			Some("Tertiary Releaser Key".to_string()),
		)
		.await
		.unwrap();

		// Find all keys for the device
		let keys = DeviceKey::find_by_device(&mut conn, device_id)
			.await
			.unwrap();
		assert_eq!(keys.len(), 3); // Initial + 2 additional

		// Verify keys have correct properties
		assert!(keys.iter().all(|k| k.device_id == device_id));
		assert!(keys.iter().all(|k| k.is_active));

		// Test that the device can still authenticate and perform releaser actions
		conn.batch_execute(
			"INSERT INTO versions (major, minor, patch, changelog, published) VALUES (4, 0, 0, 'Key management test', true)",
		)
		.await
		.unwrap();

		let response = public
			.post("/artifacts/4.0.0/mobile/android")
			.add_header("mtls-certificate", &cert)
			.text("https://example.com/download.apk")
			.await;
		response.assert_status_ok();
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn key_rotation_scenario() {
	test_server::run_with_device_auth("server", async |mut conn, cert, device_id, public, _| {
		// Simulate a key rotation scenario where a device needs to rotate its keys
		// This is common in production environments for security purposes

		use tamanu_meta::db::devices::DeviceKey;

		// Create a test server for status updates
		use diesel::{sql_query, sql_types};
		use diesel_async::RunQueryDsl;
		let server_id = uuid::Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap();
		sql_query(
			r#"
				INSERT INTO servers (id, host, kind, device_id)
				VALUES ($1, 'https://rotation-test.com', 'facility', $2)
			"#,
		)
		.bind::<sql_types::Uuid, _>(server_id)
		.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
		.execute(&mut conn)
		.await
		.expect("insert server");

		// Verify initial authentication works
		let response_initial = public
			.post("/status/99999999-9999-9999-9999-999999999999")
			.add_header("mtls-certificate", &cert)
			.json(&serde_json::json!({"uptime": 1800}))
			.await;
		response_initial.assert_status_ok();

		// Add a new key (rotation key) while keeping the old one active
		let (new_key_data, new_cert) = make_certificate();
		let _new_key = DeviceKey::create(
			&mut conn,
			device_id,
			new_key_data,
			Some("Rotated Key".to_string()),
		)
		.await
		.unwrap();

		// Verify both keys work (overlap period for graceful rotation)
		let response = public
			.post("/status/99999999-9999-9999-9999-999999999999")
			.add_header("mtls-certificate", &cert)
			.json(&serde_json::json!({"uptime": 3600}))
			.await;
		response.assert_status_ok();
		let response = public
			.post("/status/99999999-9999-9999-9999-999999999999")
			.add_header("mtls-certificate", &new_cert)
			.json(&serde_json::json!({"uptime": 5400}))
			.await;
		response.assert_status_ok();

		// Deactivate the old key
		let all_keys = DeviceKey::find_by_device(&mut conn, device_id)
			.await
			.unwrap();
		assert_eq!(all_keys.len(), 2);
		let initial_key_record = all_keys
			.iter()
			.find(|k| k.name.as_ref().map(|n| n == "Test Key").unwrap_or(false))
			.unwrap();
		DeviceKey::deactivate(&mut conn, initial_key_record.id)
			.await
			.unwrap();

		// Verify old key no longer works for authentication
		let response = public
			.post("/status/99999999-9999-9999-9999-999999999999")
			.add_header("mtls-certificate", &cert)
			.json(&serde_json::json!({"uptime": 5400}))
			.await;
		response.assert_status_not_ok();

		// Verify the new key works
		let response = public
			.post("/status/99999999-9999-9999-9999-999999999999")
			.add_header("mtls-certificate", &new_cert)
			.json(&serde_json::json!({"uptime": 5400}))
			.await;
		response.assert_status_ok();
	})
	.await;
}
