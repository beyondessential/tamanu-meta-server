use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(QueryableByName)]
struct StatusResult {
	#[diesel(sql_type = sql_types::Uuid)]
	server_id: Uuid,
	#[diesel(sql_type = sql_types::Nullable<sql_types::Uuid>)]
	device_id: Option<Uuid>,
	#[diesel(sql_type = sql_types::Jsonb)]
	extra: serde_json::Value,
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
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

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "uptime": 3600 }))
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
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
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(3600));

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
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
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(3600)
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_geolocation() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				r#"
				INSERT INTO servers (id, host, kind, device_id, geolocation)
				VALUES ($1, 'https://test.example.com', 'facility', $2, ARRAY[-41.2865, 174.7762])
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
			.execute(&mut conn)
			.await
			.expect("insert server with geolocation");

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "uptime": 7200, "version": "2.8.1" }))
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
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
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(7200));
			assert_eq!(extra.get("version").and_then(|v| v.as_str()), Some("2.8.1"));

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
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
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(7200)
			);
			assert_eq!(
				db_status.extra.get("version").and_then(|v| v.as_str()),
				Some("2.8.1")
			);

			// Verify server still has geolocation
			#[derive(QueryableByName)]
			struct GeoCheck {
				#[diesel(sql_type = sql_types::Bool)]
				has_geolocation: bool,
			}

			let server_with_geo: GeoCheck = sql_query(
				r#"
				SELECT geolocation IS NOT NULL as has_geolocation
				FROM servers
				WHERE id = $1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch server geolocation status");

			assert!(
				server_with_geo.has_geolocation,
				"Server should have geolocation"
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_cloud() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				r#"
				INSERT INTO servers (id, host, kind, device_id, cloud)
				VALUES ($1, 'https://cloud.example.com', 'central', $2, true)
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
			.execute(&mut conn)
			.await
			.expect("insert server with cloud");

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(&serde_json::json!({ "uptime": 4800, "platform": "Linux" }))
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
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
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(4800));
			assert_eq!(
				extra.get("platform").and_then(|v| v.as_str()),
				Some("Linux")
			);

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
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
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(4800)
			);
			assert_eq!(
				db_status.extra.get("platform").and_then(|v| v.as_str()),
				Some("Linux")
			);

			// Verify server still has cloud field set to true
			#[derive(QueryableByName)]
			struct CloudCheck {
				#[diesel(sql_type = sql_types::Nullable<sql_types::Bool>)]
				cloud: Option<bool>,
			}

			let server_with_cloud: CloudCheck = sql_query(
				r#"
				SELECT cloud
				FROM servers
				WHERE id = $1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch server cloud status");

			assert_eq!(
				server_with_cloud.cloud,
				Some(true),
				"Server should have cloud=true"
			);
		},
	)
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_status_with_geolocation_and_cloud() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				r#"
				INSERT INTO servers (id, host, kind, device_id, geolocation, cloud)
				VALUES ($1, 'https://full.example.com', 'central', $2, ARRAY[40.7128, -74.0060], false)
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Nullable<sql_types::Uuid>, _>(Some(device_id))
			.execute(&mut conn)
			.await
			.expect("insert server with geolocation and cloud");

			let response = public
				.post(&format!("/status/{}", server_id))
				.add_header("mtls-certificate", &cert)
				.json(
					&serde_json::json!({ "uptime": 10000, "version": "3.0.0", "timezone": "America/New_York" }),
				)
				.await;
			response.assert_status_ok();
			response.assert_header("content-type", "application/json");

			// Verify the returned status data
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
			let extra = returned_status.get("extra").expect("extra field");
			assert_eq!(extra.get("uptime").and_then(|v| v.as_i64()), Some(10000));
			assert_eq!(extra.get("version").and_then(|v| v.as_str()), Some("3.0.0"));
			assert_eq!(
				extra.get("timezone").and_then(|v| v.as_str()),
				Some("America/New_York")
			);

			// Verify the status was actually stored in the database
			let db_status: StatusResult = sql_query(
				r#"
				SELECT server_id, device_id, version, extra
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
			assert_eq!(
				db_status.extra.get("uptime").and_then(|v| v.as_i64()),
				Some(10000)
			);
			assert_eq!(
				db_status.extra.get("version").and_then(|v| v.as_str()),
				Some("3.0.0")
			);
			assert_eq!(
				db_status.extra.get("timezone").and_then(|v| v.as_str()),
				Some("America/New_York")
			);

			// Verify server still has both geolocation and cloud fields
			#[derive(QueryableByName)]
			struct FullCheck {
				#[diesel(sql_type = sql_types::Nullable<sql_types::Array<sql_types::Float8>>)]
				geolocation: Option<Vec<f64>>,
				#[diesel(sql_type = sql_types::Nullable<sql_types::Bool>)]
				cloud: Option<bool>,
			}

			let server_check: FullCheck = sql_query(
				r#"
				SELECT geolocation, cloud
				FROM servers
				WHERE id = $1
			"#,
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.get_result(&mut conn)
			.await
			.expect("fetch server geolocation and cloud status");

			assert!(
				server_check.geolocation.is_some(),
				"Server should have geolocation"
			);
			if let Some(geo) = &server_check.geolocation {
				assert_eq!(geo.len(), 2, "Geolocation should have 2 values");
				assert!(
					(geo[0] - 40.7128).abs() < 0.0001,
					"Latitude should be ~40.7128"
				);
				assert!(
					(geo[1] - (-74.0060)).abs() < 0.0001,
					"Longitude should be ~-74.0060"
				);
			}

			assert_eq!(
				server_check.cloud,
				Some(false),
				"Server should have cloud=false"
			);
		},
	)
	.await
}
