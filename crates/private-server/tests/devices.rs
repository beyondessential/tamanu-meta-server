use base64::Engine;
use database::{Device, DeviceConnection, DeviceKey, DeviceRole};

#[tokio::test(flavor = "multi_thread")]
async fn test_count_untrusted_devices() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Initially should be 0
		let count = Device::count_untrusted(&mut conn).await.unwrap();
		assert_eq!(count, 0);

		// Create 3 untrusted devices
		for i in 0..3 {
			Device::create(&mut conn, vec![i, i + 1, i + 2])
				.await
				.unwrap();
		}

		// Count should now be 3
		let count = Device::count_untrusted(&mut conn).await.unwrap();
		assert_eq!(count, 3);

		// Trust one device
		let devices = Device::list_untrusted_with_info(&mut conn).await.unwrap();
		Device::trust(&mut conn, devices[0].device.id, DeviceRole::Server)
			.await
			.unwrap();

		// Count should now be 2
		let count = Device::count_untrusted(&mut conn).await.unwrap();
		assert_eq!(count, 2);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_count_trusted_devices() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Initially should be 0
		let count = Device::count_trusted(&mut conn).await.unwrap();
		assert_eq!(count, 0);

		// Create 3 devices and trust 2 of them
		for i in 0..3 {
			let device = Device::create(&mut conn, vec![i, i + 1, i + 2])
				.await
				.unwrap();
			if i < 2 {
				Device::trust(&mut conn, device.id, DeviceRole::Server)
					.await
					.unwrap();
			}
		}

		// Count should be 2
		let count = Device::count_trusted(&mut conn).await.unwrap();
		assert_eq!(count, 2);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_list_untrusted_pagination() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create 15 untrusted devices
		for i in 0..15 {
			Device::create(&mut conn, vec![i as u8]).await.unwrap();
		}

		// Get first page (10 items)
		let page1 = Device::list_untrusted_with_info_paginated(&mut conn, 10, 0)
			.await
			.unwrap();
		assert_eq!(page1.len(), 10);

		// Get second page (5 items)
		let page2 = Device::list_untrusted_with_info_paginated(&mut conn, 10, 10)
			.await
			.unwrap();
		assert_eq!(page2.len(), 5);

		// Verify no overlap
		let page1_ids: Vec<_> = page1.iter().map(|d| d.device.id).collect();
		let page2_ids: Vec<_> = page2.iter().map(|d| d.device.id).collect();
		for id in &page1_ids {
			assert!(!page2_ids.contains(id));
		}

		// Verify ordered by created_at desc (newer first)
		for i in 1..page1.len() {
			assert!(page1[i - 1].device.created_at >= page1[i].device.created_at);
		}
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_list_trusted_pagination() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create 15 devices and trust them all
		for i in 0..15 {
			let device = Device::create(&mut conn, vec![i as u8]).await.unwrap();
			Device::trust(&mut conn, device.id, DeviceRole::Server)
				.await
				.unwrap();
		}

		// Get first page (10 items)
		let page1 = Device::list_trusted_with_info_paginated(&mut conn, 10, 0)
			.await
			.unwrap();
		assert_eq!(page1.len(), 10);

		// Get second page (5 items)
		let page2 = Device::list_trusted_with_info_paginated(&mut conn, 10, 10)
			.await
			.unwrap();
		assert_eq!(page2.len(), 5);

		// Verify no overlap
		let page1_ids: Vec<_> = page1.iter().map(|d| d.device.id).collect();
		let page2_ids: Vec<_> = page2.iter().map(|d| d.device.id).collect();
		for id in &page1_ids {
			assert!(!page2_ids.contains(id));
		}

		// Verify ordered by created_at desc (newer first)
		for i in 1..page1.len() {
			assert!(page1[i - 1].device.created_at >= page1[i].device.created_at);
		}
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_get_device_by_id() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device
		let key_data = b"test-device-key-data";
		let device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();

		// Get device by ID
		let device_info = Device::get_with_info(&mut conn, device.id).await.unwrap();

		// Verify device info
		assert_eq!(device_info.device.id, device.id);
		assert_eq!(device_info.device.role, DeviceRole::Untrusted);
		assert_eq!(device_info.keys.len(), 1);
		assert_eq!(device_info.keys[0].key_data, key_data);
		assert!(device_info.latest_connection.is_none());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_list_untrusted_devices_empty() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Test that listing untrusted devices returns empty when none exist
		let devices = Device::list_untrusted_with_info(&mut conn).await.unwrap();
		assert!(devices.is_empty());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_create_untrusted_device() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with a key
		let key_data = b"test-device-key-data";
		let device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();

		// Verify it was created as untrusted
		assert_eq!(device.role, DeviceRole::Untrusted);

		// List untrusted devices and verify it appears
		let devices = Device::list_untrusted_with_info(&mut conn).await.unwrap();
		assert_eq!(devices.len(), 1);
		assert_eq!(devices[0].device.id, device.id);
		assert_eq!(devices[0].keys.len(), 1);
		assert_eq!(devices[0].keys[0].key_data, key_data);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_trust_device() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create an untrusted device
		let key_data = b"test-device-key-data";
		let device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();

		// Trust the device as admin
		Device::trust(&mut conn, device.id, DeviceRole::Admin)
			.await
			.unwrap();

		// Verify it's no longer in the untrusted list
		let untrusted_devices = Device::list_untrusted_with_info(&mut conn).await.unwrap();
		assert!(untrusted_devices.is_empty());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_list_trusted_devices() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create two devices
		let key_data1 = b"test-device-key-data-1";
		let key_data2 = b"test-device-key-data-2";
		let device1 = Device::create(&mut conn, key_data1.to_vec()).await.unwrap();
		let device2 = Device::create(&mut conn, key_data2.to_vec()).await.unwrap();

		// Trust one device as admin and another as server
		Device::trust(&mut conn, device1.id, DeviceRole::Admin)
			.await
			.unwrap();
		Device::trust(&mut conn, device2.id, DeviceRole::Server)
			.await
			.unwrap();

		// List trusted devices and verify both appear
		let trusted_devices = Device::list_trusted_with_info(&mut conn).await.unwrap();
		assert_eq!(trusted_devices.len(), 2);

		let device_ids: Vec<_> = trusted_devices.iter().map(|d| d.device.id).collect();
		assert!(device_ids.contains(&device1.id));
		assert!(device_ids.contains(&device2.id));

		// Verify roles are correct
		for device in &trusted_devices {
			if device.device.id == device1.id {
				assert_eq!(device.device.role, DeviceRole::Admin);
			} else if device.device.id == device2.id {
				assert_eq!(device.device.role, DeviceRole::Server);
			}
		}

		// Verify untrusted list is empty
		let untrusted_devices = Device::list_untrusted_with_info(&mut conn).await.unwrap();
		assert!(untrusted_devices.is_empty());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_untrust_device() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create and trust a device
		let key_data = b"test-device-key-data";
		let device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();
		Device::trust(&mut conn, device.id, DeviceRole::Admin)
			.await
			.unwrap();

		// Verify it's in trusted list
		let trusted_devices = Device::list_trusted_with_info(&mut conn).await.unwrap();
		assert_eq!(trusted_devices.len(), 1);

		// Untrust the device
		Device::untrust(&mut conn, device.id).await.unwrap();

		// Verify it's back in untrusted list
		let untrusted_devices = Device::list_untrusted_with_info(&mut conn).await.unwrap();
		assert_eq!(untrusted_devices.len(), 1);
		assert_eq!(untrusted_devices[0].device.id, device.id);
		assert_eq!(untrusted_devices[0].device.role, DeviceRole::Untrusted);

		// Verify trusted list is empty
		let trusted_devices = Device::list_trusted_with_info(&mut conn).await.unwrap();
		assert!(trusted_devices.is_empty());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_update_device_role() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create and trust a device as server
		let key_data = b"test-device-key-data";
		let device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();
		Device::trust(&mut conn, device.id, DeviceRole::Server)
			.await
			.unwrap();

		// Update role to admin
		Device::trust(&mut conn, device.id, DeviceRole::Admin)
			.await
			.unwrap();

		// Verify role was updated
		let trusted_devices = Device::list_trusted_with_info(&mut conn).await.unwrap();
		assert_eq!(trusted_devices.len(), 1);
		assert_eq!(trusted_devices[0].device.role, DeviceRole::Admin);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_by_hex() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with specific key data
		let key_data = hex::decode("308201220a0b0c0d0e0f").unwrap();
		let device = Device::create(&mut conn, key_data.clone()).await.unwrap();

		// Search for the device using hex format (with colons)
		let search_query = "30:82:01:22";
		let results = Device::search_by_key(&mut conn, search_query)
			.await
			.unwrap();

		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);
		assert_eq!(results[0].keys[0].key_data, key_data);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_by_hex_no_colons() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with specific key data
		let key_data = hex::decode("308201220a0b0c0d0e0f").unwrap();
		let device = Device::create(&mut conn, key_data.clone()).await.unwrap();

		// Search for the device using hex format (without colons)
		let search_query = "308201220a0b";
		let results = Device::search_by_key(&mut conn, search_query)
			.await
			.unwrap();

		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_no_matches() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device
		let key_data = b"test-device-key-data";
		let _device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();

		// Search for something that doesn't exist
		let search_query = "FFFFFFFFFFFFFFFF";
		let results = Device::search_by_key(&mut conn, search_query)
			.await
			.unwrap();

		assert!(results.is_empty());
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_device_connection_history() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device
		let key_data = b"test-device-key-data";
		let device = Device::create(&mut conn, key_data.to_vec()).await.unwrap();

		// Create some connection records
		use database::devices::NewDeviceConnection;

		let connection1 = NewDeviceConnection {
			device_id: device.id,
			ip: "192.168.1.1/32".parse().unwrap(),
			user_agent: Some("Test Agent 1".to_string()),
		};
		connection1.create(&mut conn).await.unwrap();

		tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

		let connection2 = NewDeviceConnection {
			device_id: device.id,
			ip: "192.168.1.2/32".parse().unwrap(),
			user_agent: Some("Test Agent 2".to_string()),
		};
		connection2.create(&mut conn).await.unwrap();

		// Get connection history
		let history = DeviceConnection::get_history_for_device(&mut conn, device.id, 10)
			.await
			.unwrap();

		assert_eq!(history.len(), 2);
		// Should be ordered by most recent first
		assert_eq!(history[0].ip.to_string(), "192.168.1.2/32");
		assert_eq!(history[1].ip.to_string(), "192.168.1.1/32");
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_device_with_multiple_keys() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device
		let key_data1 = b"first-device-key";
		let device = Device::create(&mut conn, key_data1.to_vec()).await.unwrap();

		// Add a second key
		let key_data2 = b"second-device-key";
		DeviceKey::create(
			&mut conn,
			device.id,
			key_data2.to_vec(),
			Some("Second Key".to_string()),
		)
		.await
		.unwrap();

		// List untrusted devices and verify both keys are present
		let devices = Device::list_untrusted_with_info(&mut conn).await.unwrap();
		assert_eq!(devices.len(), 1);
		assert_eq!(devices[0].keys.len(), 2);

		// Verify key data
		let key_data_values: Vec<&[u8]> = devices[0]
			.keys
			.iter()
			.map(|k| k.key_data.as_slice())
			.collect();
		assert!(key_data_values.contains(&key_data1.as_slice()));
		assert!(key_data_values.contains(&key_data2.as_slice()));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_function_list_trusted() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create and trust some devices
		let key_data1 = b"test-device-key-data-1";
		let key_data2 = b"test-device-key-data-2";
		let device1 = Device::create(&mut conn, key_data1.to_vec()).await.unwrap();
		let device2 = Device::create(&mut conn, key_data2.to_vec()).await.unwrap();

		Device::trust(&mut conn, device1.id, DeviceRole::Admin)
			.await
			.unwrap();
		Device::trust(&mut conn, device2.id, DeviceRole::Server)
			.await
			.unwrap();

		// Test list_trusted function directly (this would normally be called via HTTP)
		// Since we're testing the database layer, this verifies the core functionality
		let trusted_devices = Device::list_trusted_with_info(&mut conn).await.unwrap();
		assert_eq!(trusted_devices.len(), 2);

		let device_ids: Vec<_> = trusted_devices.iter().map(|d| d.device.id).collect();
		assert!(device_ids.contains(&device1.id));
		assert!(device_ids.contains(&device2.id));
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_finds_any_key() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with multiple keys
		let key_data1 = hex::decode("AAAA").unwrap();
		let device = Device::create(&mut conn, key_data1.clone()).await.unwrap();

		let key_data2 = hex::decode("BBBB").unwrap();
		DeviceKey::create(
			&mut conn,
			device.id,
			key_data2.clone(),
			Some("Second Key".to_string()),
		)
		.await
		.unwrap();

		// Search for the first key
		let results1 = Device::search_by_key(&mut conn, "AA:AA").await.unwrap();
		assert_eq!(results1.len(), 1);
		assert_eq!(results1[0].device.id, device.id);

		// Search for the second key
		let results2 = Device::search_by_key(&mut conn, "BBBB").await.unwrap();
		assert_eq!(results2.len(), 1);
		assert_eq!(results2[0].device.id, device.id);

		// Both searches should return the same device with both keys
		assert_eq!(results1[0].keys.len(), 2);
		assert_eq!(results2[0].keys.len(), 2);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_by_pem_key_full() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with a PEM encoded public key
		let pem_key = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----"#;

		// Decode the PEM to get the raw key data
		let base64_part = pem_key
			.lines()
			.filter(|line| !line.starts_with("-----"))
			.collect::<Vec<_>>()
			.join("");
		let key_data = base64::prelude::BASE64_STANDARD
			.decode(base64_part)
			.unwrap();

		let device = Device::create(&mut conn, key_data.clone()).await.unwrap();

		// Search using the full PEM key
		let results = Device::search_by_key(&mut conn, pem_key).await.unwrap();

		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);
		assert_eq!(results[0].keys[0].key_data, key_data);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_by_pem_key_second() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with the second PEM encoded public key
		let pem_key = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEd72ibUgIbouPDbw2BLhfKNJuS1BB
2TpB3prr/PbmTHcuWMTeazh6lhNtbE+ryoWoaS6iFkpbyH0wYGrr0hfG9Q==
-----END PUBLIC KEY-----"#;

		// Decode the PEM to get the raw key data
		let base64_part = pem_key
			.lines()
			.filter(|line| !line.starts_with("-----"))
			.collect::<Vec<_>>()
			.join("");
		let key_data = base64::prelude::BASE64_STANDARD
			.decode(base64_part)
			.unwrap();

		let device = Device::create(&mut conn, key_data.clone()).await.unwrap();

		// Search using the full PEM key
		let results = Device::search_by_key(&mut conn, pem_key).await.unwrap();

		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);
		assert_eq!(results[0].keys[0].key_data, key_data);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_by_partial_base64() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create devices with both test keys
		let pem_key1 = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----"#;

		let pem_key2 = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEd72ibUgIbouPDbw2BLhfKNJuS1BB
2TpB3prr/PbmTHcuWMTeazh6lhNtbE+ryoWoaS6iFkpbyH0wYGrr0hfG9Q==
-----END PUBLIC KEY-----"#;

		// Decode both keys
		let key_data1 = base64::prelude::BASE64_STANDARD
			.decode(
				pem_key1
					.lines()
					.filter(|line| !line.starts_with("-----"))
					.collect::<Vec<_>>()
					.join(""),
			)
			.unwrap();
		let key_data2 = base64::prelude::BASE64_STANDARD
			.decode(
				pem_key2
					.lines()
					.filter(|line| !line.starts_with("-----"))
					.collect::<Vec<_>>()
					.join(""),
			)
			.unwrap();

		let device1 = Device::create(&mut conn, key_data1).await.unwrap();
		let device2 = Device::create(&mut conn, key_data2).await.unwrap();

		// Search for first device using unique partial base64
		let results1 = Device::search_by_key(&mut conn, "LjoyTMJN").await.unwrap();
		assert_eq!(results1.len(), 1);
		assert_eq!(results1[0].device.id, device1.id);

		// Search for second device using unique partial base64
		let results2 = Device::search_by_key(&mut conn, "KNJuS1BB").await.unwrap();
		assert_eq!(results2.len(), 1);
		assert_eq!(results2[0].device.id, device2.id);

		// Search for first device using partial base64 from middle
		let results3 = Device::search_by_key(&mut conn, "jLOL0G+pPWUL")
			.await
			.unwrap();
		assert_eq!(results3.len(), 1);
		assert_eq!(results3[0].device.id, device1.id);

		// Search for second device using unique ending pattern
		let results4 = Device::search_by_key(&mut conn, "r0hfG9Q==").await.unwrap();
		assert_eq!(results4.len(), 1);
		assert_eq!(results4[0].device.id, device2.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_by_raw_text_key() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with raw text key data (not base64 encoded)
		let text_key = "my-device-identifier-12345";
		let device = Device::create(&mut conn, text_key.as_bytes().to_vec())
			.await
			.unwrap();

		// Search for a substring that's not valid base64 (contains hyphen)
		let results = Device::search_by_key(&mut conn, "device-identifier")
			.await
			.unwrap();
		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);

		// Search for another substring with non-base64 characters
		let results2 = Device::search_by_key(&mut conn, "12345").await.unwrap();
		assert_eq!(results2.len(), 1);
		assert_eq!(results2[0].device.id, device.id);

		// Search for something not in the key
		let results3 = Device::search_by_key(&mut conn, "not-found").await.unwrap();
		assert_eq!(results3.len(), 0);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_by_base64_decoded_key() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with decoded key data from base64
		let base64_key = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJNjLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==";
		let key_data = base64::prelude::BASE64_STANDARD.decode(base64_key).unwrap();
		let device = Device::create(&mut conn, key_data.clone()).await.unwrap();

		// Test that the search function will find the device using base64 string search
		// The full base64 string should be found via PostgreSQL's encode function
		let results = Device::search_by_key(&mut conn, base64_key).await.unwrap();
		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);

		// Test with unique partial base64 string that's definitely in this key
		let partial_results = Device::search_by_key(&mut conn, "LjoyTMJN").await.unwrap();
		assert_eq!(partial_results.len(), 1);
		assert_eq!(partial_results[0].device.id, device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_pem_vs_hex_same_key() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with a key
		let pem_key = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----"#;

		let base64_part = pem_key
			.lines()
			.filter(|line| !line.starts_with("-----"))
			.collect::<Vec<_>>()
			.join("");
		let key_data = base64::prelude::BASE64_STANDARD
			.decode(base64_part)
			.unwrap();

		let device = Device::create(&mut conn, key_data.clone()).await.unwrap();

		// Search using PEM format
		let pem_results = Device::search_by_key(&mut conn, pem_key).await.unwrap();
		assert_eq!(pem_results.len(), 1);
		assert_eq!(pem_results[0].device.id, device.id);

		// Search using hex format (first few bytes)
		let hex_query = hex::encode(&key_data[..8]).to_uppercase();
		let hex_results = Device::search_by_key(&mut conn, &hex_query).await.unwrap();
		assert_eq!(hex_results.len(), 1);
		assert_eq!(hex_results[0].device.id, device.id);

		// Search using hex format with colons
		let hex_with_colons = key_data[..6]
			.iter()
			.map(|b| format!("{:02X}", b))
			.collect::<Vec<_>>()
			.join(":");
		let hex_colon_results = Device::search_by_key(&mut conn, &hex_with_colons)
			.await
			.unwrap();
		assert_eq!(hex_colon_results.len(), 1);
		assert_eq!(hex_colon_results[0].device.id, device.id);

		// All searches should return the same device
		assert_eq!(pem_results[0].device.id, hex_results[0].device.id);
		assert_eq!(hex_results[0].device.id, hex_colon_results[0].device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_multiple_pem_keys() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with multiple PEM keys
		let pem_key1 = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----"#;

		let pem_key2 = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEd72ibUgIbouPDbw2BLhfKNJuS1BB
2TpB3prr/PbmTHcuWMTeazh6lhNtbE+ryoWoaS6iFkpbyH0wYGrr0hfG9Q==
-----END PUBLIC KEY-----"#;

		// Create device with first key
		let key_data1 = base64::prelude::BASE64_STANDARD
			.decode(
				pem_key1
					.lines()
					.filter(|line| !line.starts_with("-----"))
					.collect::<Vec<_>>()
					.join(""),
			)
			.unwrap();
		let device = Device::create(&mut conn, key_data1).await.unwrap();

		// Add second key to same device
		let key_data2 = base64::prelude::BASE64_STANDARD
			.decode(
				pem_key2
					.lines()
					.filter(|line| !line.starts_with("-----"))
					.collect::<Vec<_>>()
					.join(""),
			)
			.unwrap();
		DeviceKey::create(
			&mut conn,
			device.id,
			key_data2,
			Some("Second PEM Key".to_string()),
		)
		.await
		.unwrap();

		// Search for device using first key
		let results1 = Device::search_by_key(&mut conn, pem_key1).await.unwrap();
		assert_eq!(results1.len(), 1);
		assert_eq!(results1[0].device.id, device.id);
		assert_eq!(results1[0].keys.len(), 2);

		// Search for device using second key
		let results2 = Device::search_by_key(&mut conn, pem_key2).await.unwrap();
		assert_eq!(results2.len(), 1);
		assert_eq!(results2[0].device.id, device.id);
		assert_eq!(results2[0].keys.len(), 2);

		// Both searches should return the same device with both keys
		assert_eq!(results1[0].device.id, results2[0].device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_invalid_base64_fallback() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with text-based key data
		let text_key = b"this-is-not-base64-or-hex-data";
		let device = Device::create(&mut conn, text_key.to_vec()).await.unwrap();

		// Search using partial text (should fallback to byte search)
		let results = Device::search_by_key(&mut conn, "not-base64-or")
			.await
			.unwrap();

		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);
		assert_eq!(results[0].keys[0].key_data, text_key);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_case_insensitive_hex() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with known hex data
		let key_data = hex::decode("DEADBEEFCAFE1234").unwrap();
		let device = Device::create(&mut conn, key_data.clone()).await.unwrap();

		// Search using lowercase hex
		let results_lower = Device::search_by_key(&mut conn, "deadbeef").await.unwrap();
		assert_eq!(results_lower.len(), 1);
		assert_eq!(results_lower[0].device.id, device.id);

		// Search using uppercase hex
		let results_upper = Device::search_by_key(&mut conn, "DEADBEEF").await.unwrap();
		assert_eq!(results_upper.len(), 1);
		assert_eq!(results_upper[0].device.id, device.id);

		// Search using mixed case hex
		let results_mixed = Device::search_by_key(&mut conn, "DeAdBeEf").await.unwrap();
		assert_eq!(results_mixed.len(), 1);
		assert_eq!(results_mixed[0].device.id, device.id);

		// All should find the same device
		assert_eq!(results_lower[0].device.id, results_upper[0].device.id);
		assert_eq!(results_upper[0].device.id, results_mixed[0].device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_malformed_pem() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with a proper key
		let proper_pem_key = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----"#;

		let key_data = base64::prelude::BASE64_STANDARD
			.decode(
				proper_pem_key
					.lines()
					.filter(|line| !line.starts_with("-----"))
					.collect::<Vec<_>>()
					.join(""),
			)
			.unwrap();
		let device = Device::create(&mut conn, key_data).await.unwrap();

		// Search with malformed PEM (missing END header)
		let malformed_pem1 = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w=="#;

		let results1 = Device::search_by_key(&mut conn, malformed_pem1)
			.await
			.unwrap();
		// Won't find the device because it requires both BEGIN and END headers to be recognized as PEM
		assert_eq!(results1.len(), 0);

		// Search with malformed PEM (missing BEGIN header) - will try base64 decode of the whole thing
		let malformed_pem2 = r#"MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----"#;

		let results2 = Device::search_by_key(&mut conn, malformed_pem2)
			.await
			.unwrap();
		// Won't find because the multi-line format with header can't be decoded as base64
		assert_eq!(results2.len(), 0);

		// But searching with a unique pattern from the base64 should work
		let results3 = Device::search_by_key(&mut conn, "LjoyTMJN").await.unwrap();
		assert_eq!(results3.len(), 1);
		assert_eq!(results3[0].device.id, device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_pem_with_whitespace() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with a key
		let base64_content = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJNjLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==";
		let key_data = base64::prelude::BASE64_STANDARD.decode(base64_content).unwrap();
		let device = Device::create(&mut conn, key_data).await.unwrap();

		// Search with PEM containing extra whitespace and line breaks
		// The extra indentation means the base64 lines have leading tabs/spaces
		// which will cause base64 decode to fail, so it will fall back to raw bytes
		let pem_with_whitespace = r#"
		-----BEGIN PUBLIC KEY-----
		MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
		jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
		-----END PUBLIC KEY-----
		"#;

		let results = Device::search_by_key(&mut conn, pem_with_whitespace).await.unwrap();
		// This won't match because the indented base64 can't be decoded properly
		assert_eq!(results.len(), 0);

		// However, a properly formatted PEM (even with surrounding whitespace) should work
		let proper_pem = r#"
-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----
"#;

		let proper_results = Device::search_by_key(&mut conn, proper_pem).await.unwrap();
		assert_eq!(proper_results.len(), 1);
		assert_eq!(proper_results[0].device.id, device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_short_partial_matches() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create devices with the test keys
		let key1_base64 = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJNjLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==";
		let key2_base64 = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEd72ibUgIbouPDbw2BLhfKNJuS1BB2TpB3prr/PbmTHcuWMTeazh6lhNtbE+ryoWoaS6iFkpbyH0wYGrr0hfG9Q==";

		let key_data1 = base64::prelude::BASE64_STANDARD.decode(key1_base64).unwrap();
		let key_data2 = base64::prelude::BASE64_STANDARD.decode(key2_base64).unwrap();

		let device1 = Device::create(&mut conn, key_data1.clone()).await.unwrap();
		let device2 = Device::create(&mut conn, key_data2.clone()).await.unwrap();

		// Search with unique base64 patterns specific to each key
		let results1 = Device::search_by_key(&mut conn, "LjoyTMJN").await.unwrap();
		assert_eq!(results1.len(), 1);
		assert_eq!(results1[0].device.id, device1.id);

		let results2 = Device::search_by_key(&mut conn, "KNJuS1BB").await.unwrap();
		assert_eq!(results2.len(), 1);
		assert_eq!(results2[0].device.id, device2.id);

		// Search with ending patterns unique to each key
		let results3 = Device::search_by_key(&mut conn, "hkp/k9w==").await.unwrap();
		assert_eq!(results3.len(), 1);
		assert_eq!(results3[0].device.id, device1.id);

		let results4 = Device::search_by_key(&mut conn, "r0hfG9Q==").await.unwrap();
		assert_eq!(results4.len(), 1);
		assert_eq!(results4[0].device.id, device2.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_pem_newlines_as_spaces() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Create a device with a key
		let pem_key = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN
jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==
-----END PUBLIC KEY-----"#;

		let base64_part = pem_key
			.lines()
			.filter(|line| !line.starts_with("-----"))
			.collect::<Vec<_>>()
			.join("");
		let key_data = base64::prelude::BASE64_STANDARD
			.decode(base64_part)
			.unwrap();
		let device = Device::create(&mut conn, key_data).await.unwrap();

		// Simulate pasting PEM into a text input field where newlines become spaces
		let pem_with_spaces = "-----BEGIN PUBLIC KEY----- MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w== -----END PUBLIC KEY-----";

		let results = Device::search_by_key(&mut conn, pem_with_spaces).await.unwrap();
		assert_eq!(results.len(), 1);
		assert_eq!(results[0].device.id, device.id);

		// Also test with extra spaces around headers
		let pem_extra_spaces = " -----BEGIN PUBLIC KEY-----  MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJN  jLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==  -----END PUBLIC KEY-----  ";

		let results2 = Device::search_by_key(&mut conn, pem_extra_spaces).await.unwrap();
		assert_eq!(results2.len(), 1);
		assert_eq!(results2[0].device.id, device.id);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_search_devices_overlapping_base64_patterns() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Both keys start with the same base64 prefix (ECDSA P-256 header)
		let key1_base64 = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEqX+XKR2cFY3JOYT83S0/LjoyTMJNjLOL0G+pPWULJ17eyFMFfxIESidq5+UpELfvOjolh0kcsMgh4J+hkp/k9w==";
		let key2_base64 = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEd72ibUgIbouPDbw2BLhfKNJuS1BB2TpB3prr/PbmTHcuWMTeazh6lhNtbE+ryoWoaS6iFkpbyH0wYGrr0hfG9Q==";

		let key_data1 = base64::prelude::BASE64_STANDARD.decode(key1_base64).unwrap();
		let key_data2 = base64::prelude::BASE64_STANDARD.decode(key2_base64).unwrap();

		let device1 = Device::create(&mut conn, key_data1).await.unwrap();
		let device2 = Device::create(&mut conn, key_data2).await.unwrap();

		// Search with common prefix (should return both devices)
		let common_prefix_results = Device::search_by_key(&mut conn, "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE").await.unwrap();
		assert_eq!(common_prefix_results.len(), 2);
		let device_ids: Vec<_> = common_prefix_results.iter().map(|d| d.device.id).collect();
		assert!(device_ids.contains(&device1.id));
		assert!(device_ids.contains(&device2.id));

		// Search with unique parts to distinguish them
		let unique1_results = Device::search_by_key(&mut conn, "LjoyTMJN").await.unwrap();
		assert_eq!(unique1_results.len(), 1);
		assert_eq!(unique1_results[0].device.id, device1.id);

		let unique2_results = Device::search_by_key(&mut conn, "KNJuS1BB").await.unwrap();
		assert_eq!(unique2_results.len(), 1);
		assert_eq!(unique2_results[0].device.id, device2.id);
	})
	.await;
}
