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
