use database::{Device, DeviceConnection, DeviceKey, DeviceRole};

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
